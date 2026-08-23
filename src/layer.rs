//! Tower integration for request-scoped response metadata.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;

use http::{Request, Response};
use http_body::Body;
use tower::{Layer, Service};

use crate::{EnvelopeBody, MetadataError, ResponseMetadata, wrap_response};

/// A Tower layer that requires [`ResponseMetadata`] in request extensions and
/// applies it to every successful response from the wrapped service.
#[derive(Clone, Copy, Debug, Default)]
pub struct StreamingEnvelopeLayer;

impl StreamingEnvelopeLayer {
    /// Construct the layer.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for StreamingEnvelopeLayer {
    type Service = StreamingEnvelopeService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        StreamingEnvelopeService { inner }
    }
}

/// The service produced by [`StreamingEnvelopeLayer`].
#[derive(Clone, Debug)]
pub struct StreamingEnvelopeService<S> {
    pub(crate) inner: S,
}

/// A fail-fast Tower layer for Axum and other services that require an
/// infallible service error.
#[derive(Clone, Copy, Debug, Default)]
pub struct InfallibleStreamingEnvelopeLayer;

impl InfallibleStreamingEnvelopeLayer {
    /// Construct the fail-fast layer.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for InfallibleStreamingEnvelopeLayer {
    type Service = InfallibleStreamingEnvelopeService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        InfallibleStreamingEnvelopeService { inner }
    }
}

/// The service produced by [`InfallibleStreamingEnvelopeLayer`].
#[derive(Clone, Debug)]
pub struct InfallibleStreamingEnvelopeService<S> {
    inner: S,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for StreamingEnvelopeService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + 'static,
    S::Future: 'static,
    ReqBody: 'static,
    ResBody: Body + 'static,
{
    type Response = Response<EnvelopeBody<ResBody>>;
    type Error = EnvelopeServiceError<S::Error>;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(EnvelopeServiceError::Inner)
    }

    fn call(&mut self, request: Request<ReqBody>) -> Self::Future {
        let Some(metadata) = request.extensions().get::<ResponseMetadata>().cloned() else {
            return Box::pin(async { Err(EnvelopeServiceError::MissingMetadata) });
        };
        let future = self.inner.call(request);
        Box::pin(async move {
            let response = future.await.map_err(EnvelopeServiceError::Inner)?;
            wrap_response(response, &metadata).map_err(EnvelopeServiceError::Metadata)
        })
    }
}

/// Errors produced by the Tower envelope layer.
#[derive(Debug, thiserror::Error)]
pub enum EnvelopeServiceError<E> {
    /// The request did not carry the mandatory response metadata extension.
    #[error("request is missing mandatory ResponseMetadata extension")]
    MissingMetadata,
    /// The wrapped service returned an error.
    #[error("wrapped service failed")]
    Inner(E),
    /// Response metadata could not be applied.
    #[error(transparent)]
    Metadata(#[from] MetadataError),
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for InfallibleStreamingEnvelopeService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<Infallible> + Send + 'static,
    ReqBody: Send + 'static,
    ResBody: Body + Send + 'static,
    ResBody::Data: Send + 'static,
    ResBody::Error: Send + 'static,
{
    type Response = Response<EnvelopeBody<ResBody>>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, request: Request<ReqBody>) -> Self::Future {
        let metadata = request
            .extensions()
            .get::<ResponseMetadata>()
            .cloned()
            .unwrap_or_else(|| panic!("mandatory ResponseMetadata extension is missing"));
        let future = self.inner.call(request);
        Box::pin(async move {
            let response = future.await.map_err(Into::into)?;
            Ok(wrap_response(response, &metadata).unwrap_or_else(|error| {
                panic!("streaming response envelope metadata failed: {error}");
            }))
        })
    }
}
