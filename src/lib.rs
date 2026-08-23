#![doc = include_str!("../README.md")]

//! The HTTP status line and response headers are the envelope's start
//! section. The wrapped body remains the payload stream, and one completion
//! trailer is emitted after the body finishes successfully.

mod body;
mod layer;
mod metadata;

pub use body::{EnvelopeBody, HEADER_PAYLOAD_BYTES, wrap_response};
pub use layer::{EnvelopeServiceError, StreamingEnvelopeLayer, StreamingEnvelopeService};
pub use metadata::{
    HEADER_GENERATED_AT, HEADER_PAYLOAD_SOURCE, HEADER_REQUEST_ID, HEADER_TRACEPARENT,
    PayloadSource, RequestId, ResponseMetadata, TraceParent,
};

/// Errors raised while validating or applying envelope metadata.
#[derive(Debug, thiserror::Error)]
pub enum MetadataError {
    /// A required identifier or metadata value was empty or malformed.
    #[error("invalid {field} metadata")]
    InvalidValue {
        /// The metadata field that failed validation.
        field: &'static str,
    },
    /// A response already contains a header owned by this crate with a
    /// different value.
    #[error("response already contains envelope header {0}")]
    ConflictingHeader(http::header::HeaderName),
}
