//! Tower integration for request-scoped response metadata.

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
#[derive(Debug)]
pub struct StreamingEnvelopeService<S> {
    pub(crate) inner: S,
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
