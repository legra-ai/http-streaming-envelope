//! Integration tests for the streaming HTTP envelope.

use std::time::SystemTime;

use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use http_streaming_envelope::{
    HEADER_PAYLOAD_BYTES, HEADER_PAYLOAD_SOURCE, HEADER_REQUEST_ID, PayloadSource, RequestId,
    ResponseMetadata, StreamingEnvelopeLayer, wrap_response,
};
use tower::{Layer, Service, ServiceExt, service_fn};

fn metadata() -> ResponseMetadata {
    ResponseMetadata::new(
        RequestId::new("req-42").expect("valid request id"),
        PayloadSource::Cache,
        SystemTime::UNIX_EPOCH,
    )
}

#[tokio::test]
async fn preserves_payload_and_emits_completion_trailer() {
    let response = Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .body(Full::new(Bytes::from_static(b"hello")))
        .expect("response");
    let metadata = metadata();
    let response = wrap_response(response, &metadata).expect("wrapped response");

    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers()[HEADER_REQUEST_ID], "req-42");
    assert_eq!(response.headers()[HEADER_PAYLOAD_SOURCE], "cache");

    let mut body = response.into_body();
    let payload = body
        .frame()
        .await
        .expect("payload frame")
        .expect("payload result")
        .into_data()
        .expect("data frame");
    assert_eq!(payload, Bytes::from_static(b"hello"));

    let trailers = body
        .frame()
        .await
        .expect("completion frame")
        .expect("completion result")
        .into_trailers()
        .expect("trailers frame");
    assert_eq!(trailers[HEADER_PAYLOAD_BYTES], "5");
    assert!(body.frame().await.is_none());
}

#[tokio::test]
async fn layer_requires_metadata_and_wraps_successful_responses() {
    let service = service_fn(|_request: Request<()>| async {
        Ok::<_, std::convert::Infallible>(Response::new(Full::new(Bytes::from_static(b"body"))))
    });
    let mut service = StreamingEnvelopeLayer::new().layer(service);

    let missing = service
        .ready()
        .await
        .expect("ready")
        .call(Request::new(()))
        .await;
    assert!(missing.is_err());

    let mut request = Request::new(());
    request.extensions_mut().insert(metadata());
    let response = service
        .ready()
        .await
        .expect("ready")
        .call(request)
        .await
        .expect("wrapped response");
    assert_eq!(response.headers()[HEADER_REQUEST_ID], "req-42");
}

#[test]
fn identifiers_are_validated_before_they_reach_headers() {
    assert!(RequestId::new("").is_err());
    assert!(RequestId::new("contains\nnewline").is_err());
    assert!(http_streaming_envelope::TraceParent::new("not-a-traceparent").is_err());
}

#[test]
fn existing_envelope_headers_are_not_overwritten() {
    let response = Response::builder()
        .header(HEADER_REQUEST_ID, "another-request")
        .body(Full::new(Bytes::from_static(b"body")))
        .expect("response");
    let metadata = metadata();
    let result = wrap_response(response, &metadata);
    assert!(matches!(
        result,
        Err(http_streaming_envelope::MetadataError::ConflictingHeader(name))
            if name == HEADER_REQUEST_ID
    ));
}
