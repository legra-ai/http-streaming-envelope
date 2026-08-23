//! Typed metadata carried before the payload stream.

use std::time::SystemTime;

use http::header::{HeaderMap, HeaderName, HeaderValue};

use crate::MetadataError;

/// The request correlation identifier header.
pub const HEADER_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");
/// The W3C trace context header.
pub const HEADER_TRACEPARENT: HeaderName = HeaderName::from_static("traceparent");
/// The source of the payload, such as a cache or fresh generation.
pub const HEADER_PAYLOAD_SOURCE: HeaderName = HeaderName::from_static("streaming-envelope-source");
/// The time at which the response representation was generated.
pub const HEADER_GENERATED_AT: HeaderName =
    HeaderName::from_static("streaming-envelope-generated-at");

/// A validated request correlation identifier.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RequestId(String);

impl RequestId {
    /// Validate and construct a request identifier.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError::InvalidValue`] when the value is empty or
    /// cannot be represented as an HTTP header value.
    pub fn new(value: impl Into<String>) -> Result<Self, MetadataError> {
        let value = value.into();
        validate_header_value("request_id", &value)?;
        Ok(Self(value))
    }

    /// Borrow the identifier as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A validated W3C `traceparent` value.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TraceParent(String);

impl TraceParent {
    /// Validate and construct a W3C `traceparent` value.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError::InvalidValue`] when the value is not a valid
    /// 55-character W3C trace context.
    pub fn new(value: impl Into<String>) -> Result<Self, MetadataError> {
        let value = value.into();
        if !is_traceparent(&value) {
            return Err(MetadataError::InvalidValue {
                field: "traceparent",
            });
        }
        Ok(Self(value))
    }

    /// Borrow the trace context as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Where the payload representation came from.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PayloadSource {
    /// The representation was generated for this request.
    Generated,
    /// The representation was served from a cache.
    Cache,
    /// The representation was obtained from another service or store.
    Upstream,
}

impl PayloadSource {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Generated => "generated",
            Self::Cache => "cache",
            Self::Upstream => "upstream",
        }
    }
}

/// Metadata that is available before the payload starts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResponseMetadata {
    request_id: RequestId,
    traceparent: Option<TraceParent>,
    payload_source: PayloadSource,
    generated_at: SystemTime,
}

impl ResponseMetadata {
    /// Construct metadata with the required request identifier, payload
    /// source, and generation time.
    #[must_use]
    pub fn new(
        request_id: RequestId,
        payload_source: PayloadSource,
        generated_at: SystemTime,
    ) -> Self {
        Self {
            request_id,
            traceparent: None,
            payload_source,
            generated_at,
        }
    }

    /// Construct metadata stamped with the current system time.
    #[must_use]
    pub fn now(request_id: RequestId, payload_source: PayloadSource) -> Self {
        Self::new(request_id, payload_source, SystemTime::now())
    }

    /// Attach a W3C trace context.
    #[must_use]
    pub fn with_traceparent(mut self, traceparent: TraceParent) -> Self {
        self.traceparent = Some(traceparent);
        self
    }

    /// Return the request identifier.
    #[must_use]
    pub fn request_id(&self) -> &RequestId {
        &self.request_id
    }

    /// Return the payload source.
    #[must_use]
    pub const fn payload_source(&self) -> PayloadSource {
        self.payload_source
    }

    /// Apply the start metadata to response headers.
    pub(crate) fn apply(&self, headers: &mut HeaderMap) -> Result<(), MetadataError> {
        insert_owned_header(headers, HEADER_REQUEST_ID, self.request_id.as_str())?;
        insert_owned_header(headers, HEADER_PAYLOAD_SOURCE, self.payload_source.as_str())?;
        insert_owned_header(
            headers,
            HEADER_GENERATED_AT,
            &httpdate::fmt_http_date(self.generated_at),
        )?;
        if let Some(traceparent) = &self.traceparent {
            insert_owned_header(headers, HEADER_TRACEPARENT, traceparent.as_str())?;
        }
        Ok(())
    }
}

fn insert_owned_header(
    headers: &mut HeaderMap,
    name: HeaderName,
    value: &str,
) -> Result<(), MetadataError> {
    if headers.contains_key(&name) {
        return Err(MetadataError::ConflictingHeader(name));
    }
    let value = HeaderValue::from_str(value).map_err(|_| MetadataError::InvalidValue {
        field: "header value",
    })?;
    headers.insert(name, value);
    Ok(())
}

fn validate_header_value(field: &'static str, value: &str) -> Result<(), MetadataError> {
    if value.is_empty() || HeaderValue::from_str(value).is_err() {
        return Err(MetadataError::InvalidValue { field });
    }
    Ok(())
}

fn is_traceparent(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 55
        && bytes[2] == b'-'
        && bytes[35] == b'-'
        && bytes[52] == b'-'
        && bytes[..2].iter().all(u8::is_ascii_hexdigit)
        && bytes[3..35].iter().all(u8::is_ascii_hexdigit)
        && bytes[36..52].iter().all(u8::is_ascii_hexdigit)
        && bytes[53..].iter().all(u8::is_ascii_hexdigit)
}
