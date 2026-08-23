//! Payload-preserving body wrapper and completion trailers.

use std::task::{Context, Poll};

use bytes::Buf;
use http::header::{HeaderMap, HeaderValue, TRAILER};
use http_body::{Body, Frame};
use pin_project_lite::pin_project;

use crate::{MetadataError, ResponseMetadata};

/// The completion trailer containing the exact number of payload bytes
/// observed by the wrapper.
pub const HEADER_PAYLOAD_BYTES: http::header::HeaderName =
    http::header::HeaderName::from_static("streaming-envelope-payload-bytes");

pin_project! {
    /// A streaming response body that preserves every payload frame and
    /// emits completion metadata only after the inner body completes.
    pub struct EnvelopeBody<B> {
        #[pin]
        inner: B,
        payload_bytes: u64,
        completion_emitted: bool,
    }
}

impl<B> EnvelopeBody<B> {
    pub(crate) fn new(inner: B) -> Self {
        Self {
            inner,
            payload_bytes: 0,
            completion_emitted: false,
        }
    }
}

impl<B> Body for EnvelopeBody<B>
where
    B: Body,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let mut this = self.project();
        match this.inner.as_mut().poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => match frame.into_data() {
                Ok(data) => {
                    *this.payload_bytes =
                        this.payload_bytes.saturating_add(data.remaining() as u64);
                    Poll::Ready(Some(Ok(Frame::data(data))))
                }
                Err(frame) => {
                    let Ok(mut trailers) = frame.into_trailers() else {
                        unreachable!("HTTP body frames are either data or trailers");
                    };
                    add_completion_trailer(&mut trailers, *this.payload_bytes);
                    *this.completion_emitted = true;
                    Poll::Ready(Some(Ok(Frame::trailers(trailers))))
                }
            },
            Poll::Ready(None) if !*this.completion_emitted => {
                let mut trailers = HeaderMap::new();
                add_completion_trailer(&mut trailers, *this.payload_bytes);
                *this.completion_emitted = true;
                Poll::Ready(Some(Ok(Frame::trailers(trailers))))
            }
            other => other,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.completion_emitted && self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}

/// Wrap a response without changing its payload representation or collecting
/// any body data.
///
/// # Errors
///
/// Returns [`MetadataError::InvalidValue`] for metadata that cannot be placed
/// in an HTTP header, or [`MetadataError::ConflictingHeader`] when the
/// response already owns one of the envelope headers.
pub fn wrap_response<B>(
    mut response: http::Response<B>,
    metadata: &ResponseMetadata,
) -> Result<http::Response<EnvelopeBody<B>>, MetadataError>
where
    B: Body,
{
    metadata.apply(response.headers_mut())?;
    response.headers_mut().append(
        TRAILER,
        HeaderValue::from_static("streaming-envelope-payload-bytes"),
    );
    Ok(response.map(EnvelopeBody::new))
}

fn add_completion_trailer(headers: &mut HeaderMap, payload_bytes: u64) {
    headers.insert(
        HEADER_PAYLOAD_BYTES,
        HeaderValue::from_str(&payload_bytes.to_string())
            .expect("u64 decimal representation is a valid header value"),
    );
}
