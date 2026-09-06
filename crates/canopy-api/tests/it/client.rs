//! The call plumbing every generated method routes through.

use bes_canopy_api::{CanopyClient, Error};

use crate::support::Recorder;

#[tokio::test]
async fn get_sends_a_path_only_uri_and_no_body() {
	let recorder = Recorder::json(200, "{}");
	let client = CanopyClient::new(recorder);

	client
		.status_check_severities("a-server")
		.await
		.expect("a 200 with an empty map parses");

	let request = client.transport().last();
	assert_eq!(request.method(), http::Method::GET);
	assert_eq!(request.uri(), "/status/a-server/check-severities");
	assert!(
		request.uri().scheme().is_none() && request.uri().authority().is_none(),
		"resolving the path against a base URL is the transport's job"
	);
	assert!(request.body().is_empty());
}

#[tokio::test]
async fn a_path_parameter_is_substituted() {
	let recorder = Recorder::json(200, "[]");
	let client = CanopyClient::new(recorder);

	client
		.versions_artifacts("2.11.0")
		.await
		.expect("a 200 with an empty list parses");

	assert_eq!(
		client.transport().last().uri(),
		"/versions/2.11.0/artifacts"
	);
}

#[tokio::test]
async fn an_unsuccessful_status_carries_the_status_and_body() {
	let recorder = Recorder::json(412, "device is dormant");
	let client = CanopyClient::new(recorder);

	let err = client
		.status_check_severities("a-server")
		.await
		.expect_err("a 412 is not a success");

	let http = err.http().expect("a non-2xx surfaces as an HTTP error");
	assert_eq!(http.status, http::StatusCode::PRECONDITION_FAILED);
	assert_eq!(http.path, "/status/a-server/check-severities");
	assert_eq!(http.body_text(), "device is dormant");
	assert_eq!(err.status(), Some(http::StatusCode::PRECONDITION_FAILED));
}

#[tokio::test]
async fn a_body_that_is_not_the_declared_json_is_a_decode_error_not_an_http_error() {
	let recorder = Recorder::json(200, "not json at all");
	let client = CanopyClient::new(recorder);

	let err = client
		.status_check_severities("a-server")
		.await
		.expect_err("a 200 carrying junk does not parse");

	assert!(matches!(err, Error::Decode { .. }));
	assert!(err.http().is_none());
}

#[tokio::test]
async fn a_transport_failure_is_distinct_from_a_failing_response() {
	use bes_canopy_api::{CanopyRequest, CanopyResponse, CanopyTransport, Result, async_trait};

	struct Unreachable;

	#[async_trait]
	impl CanopyTransport for Unreachable {
		async fn call(&self, _: CanopyRequest) -> Result<CanopyResponse> {
			Err(Error::transport(std::io::Error::other(
				"connection refused",
			)))
		}
	}

	let err = CanopyClient::new(Unreachable)
		.status_check_severities("a-server")
		.await
		.expect_err("no response was obtained");

	assert!(matches!(err, Error::Transport(_)));
	assert!(
		err.status().is_none(),
		"a failure to reach canopy carries no status"
	);
}

#[tokio::test]
async fn a_small_request_body_is_sent_uncompressed() {
	let recorder = Recorder::json(200, r#"{"ok":true}"#);
	let client = CanopyClient::new(recorder);

	let payload = bes_canopy_api::schema::StatusPayload::builder()
		.health(vec![])
		.build();
	let _ = client.status("a-server", &payload).await;

	let request = client.transport().last();
	assert_eq!(
		request.headers().get(http::header::CONTENT_TYPE).unwrap(),
		"application/json"
	);
	assert!(
		request
			.headers()
			.get(http::header::CONTENT_ENCODING)
			.is_none(),
		"a body this small costs more to compress than it saves"
	);
	assert_eq!(
		serde_json::from_slice::<serde_json::Value>(request.body()).expect("plain JSON"),
		serde_json::to_value(&payload).expect("serialising the payload")
	);
}

#[tokio::test]
async fn a_large_request_body_is_gzipped_and_says_so() {
	use std::io::Read as _;

	let recorder = Recorder::json(200, r#"{"ok":true}"#);
	let client = CanopyClient::new(recorder);

	// Well past the threshold, so the branch is taken regardless of how JSON
	// serialisation rounds out.
	let mut extra = serde_json::Map::new();
	for i in 0..200 {
		extra.insert(format!("key_number_{i}"), serde_json::json!("some value"));
	}
	let payload = bes_canopy_api::schema::StatusPayload::builder()
		.health(vec![])
		.extra(extra)
		.build();
	let _ = client.status("a-server", &payload).await;

	let request = client.transport().last();
	assert_eq!(
		request
			.headers()
			.get(http::header::CONTENT_ENCODING)
			.unwrap(),
		"gzip"
	);

	let mut decoded = Vec::new();
	flate2::read::GzDecoder::new(request.body().as_ref())
		.read_to_end(&mut decoded)
		.expect("the body is gzip");
	assert_eq!(
		serde_json::from_slice::<serde_json::Value>(&decoded).expect("the JSON inside"),
		serde_json::to_value(&payload).expect("serialising the payload")
	);
}
