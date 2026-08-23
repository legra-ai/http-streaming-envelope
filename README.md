# http-streaming-envelope

[![Crates.io](https://img.shields.io/crates/v/http-streaming-envelope.svg)](https://crates.io/crates/http-streaming-envelope)
[![Documentation](https://docs.rs/http-streaming-envelope/badge.svg)](https://docs.rs/http-streaming-envelope)
[![CI](https://github.com/legra-ai/http-streaming-envelope/actions/workflows/ci.yml/badge.svg)](https://github.com/legra-ai/http-streaming-envelope/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

HTTP-native response metadata for payloads that must remain asynchronous and
streaming.

This crate does not serialize a payload into an envelope object. It preserves
the response body exactly as a stream:

- the HTTP status and response headers are the before-payload section;
- typed request, trace, source, and generation metadata is added to headers;
- the original body frames pass through without collection or chunking;
- a completion trailer reports the exact number of payload bytes after the
  body finishes successfully.

That makes the contract suitable for JSON, JSONL, RDF, SPARQL results, binary
files, and other representations without assuming that the payload fits in
memory or that it is even self-describing.

## Why an HTTP-native envelope?

A JSON object such as `{ "payload": ... }` cannot wrap a petabyte-scale
payload without either buffering it or imposing format-specific framing. HTTP
already provides an ordered start section (status and headers), a streaming
body, and an optional completion section (trailers). This crate uses those
native boundaries instead of inventing a second whole-body protocol.

The payload source is explicit, so clients can distinguish a freshly generated
representation from one served by a cache or obtained from an upstream
service. The W3C `traceparent` value can connect the response to its task tree.
The request identifier is mandatory.

## Direct use

```rust
use std::time::SystemTime;

use bytes::Bytes;
use http::{Response, StatusCode};
use http_body_util::Full;
use http_streaming_envelope::{
    PayloadSource, RequestId, ResponseMetadata, TraceParent, wrap_response,
};

let request_id = RequestId::new("req-42")?;
let traceparent = TraceParent::new(
    "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
)?;
let metadata = ResponseMetadata::new(
    request_id,
    PayloadSource::Generated,
    SystemTime::UNIX_EPOCH,
)
.with_traceparent(traceparent);

let response = Response::builder()
    .status(StatusCode::OK)
    .body(Full::new(Bytes::from_static(b"streamed payload")))?;
let response = wrap_response(response, &metadata)?;

# Ok::<(), Box<dyn std::error::Error>>(())
```

`wrap_response` never reads the body. When the body completes, the wrapped
body emits `streaming-envelope-payload-bytes` as a trailer. If the body fails,
the original body error is returned and no false completion metadata is sent.

The HTTP status remains the canonical status. It is already delivered before
the payload and is therefore not duplicated into a second application field
that could disagree with the actual response status.

## Tower integration

Install [`ResponseMetadata`] in request extensions at the request boundary and
add [`StreamingEnvelopeLayer`] around the service:

```rust,ignore
use http::Request;
use http_streaming_envelope::{
    PayloadSource, RequestId, ResponseMetadata, StreamingEnvelopeLayer,
};
use std::time::SystemTime;
use tower::Layer;

let metadata = ResponseMetadata::new(
    RequestId::new("req-42")?,
    PayloadSource::Cache,
    SystemTime::now(),
);
let mut request = Request::new(());
request.extensions_mut().insert(metadata);
let service = StreamingEnvelopeLayer::new().layer(your_service);

# Ok::<(), Box<dyn std::error::Error>>(())
```

The layer fails fast when the mandatory metadata extension is absent. It does
not create a request identifier, guess the payload source, or silently remove
the envelope when metadata is unavailable.

Axum routers require an infallible layer service. For that boundary, use
[`InfallibleStreamingEnvelopeLayer`]. It panics on a missing metadata
extension or an invalid response header instead of converting a programming
error into a silent fallback. General Tower services can use
[`StreamingEnvelopeLayer`] when they need typed layer errors.

## Relationship to content negotiation

Use [`http-content-negotiation`](https://crates.io/crates/http-content-negotiation)
to select the representation from `Accept` and `Accept-Language`. Then use
this crate to carry response metadata around the selected representation. The
two concerns remain separate: negotiation chooses a representation, while
this crate preserves and annotates its stream.

## Scope

This crate deliberately does not provide serializers, translation catalogs,
application error codes, authentication, cache implementation, or a universal
JSON schema. Applications remain responsible for choosing those policies.

## License

Licensed under either of:

- MIT license (`LICENSE-MIT` or <https://opensource.org/licenses/MIT>);
- Apache License, Version 2.0 (`LICENSE-APACHE` or <https://www.apache.org/licenses/LICENSE-2.0>).

Copyright © 2026 `DataRoad` Inc, Delaware, USA, trading as Legra.
