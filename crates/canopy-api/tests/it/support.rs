//! A transport that records what it was handed and replays a canned response.

use std::sync::Mutex;

use bes_canopy_api::{
	CanopyRequest, CanopyResponse, CanopyTransport, Result, async_trait, bytes::Bytes,
};

pub struct Recorder {
	pub response: Mutex<Option<CanopyResponse>>,
	pub seen: Mutex<Vec<CanopyRequest>>,
}

impl Recorder {
	pub fn json(status: u16, body: &str) -> Self {
		Self {
			response: Mutex::new(Some(
				http::Response::builder()
					.status(status)
					.body(Bytes::from(body.to_owned()))
					.expect("building the canned response"),
			)),
			seen: Mutex::new(Vec::new()),
		}
	}

	pub fn last(&self) -> CanopyRequest {
		self.seen
			.lock()
			.expect("the recorder is not poisoned")
			.pop()
			.expect("a request was sent")
	}
}

#[async_trait]
impl CanopyTransport for Recorder {
	async fn call(&self, request: CanopyRequest) -> Result<CanopyResponse> {
		self.seen
			.lock()
			.expect("the recorder is not poisoned")
			.push(request);
		Ok(self
			.response
			.lock()
			.expect("the recorder is not poisoned")
			.take()
			.expect("only one call per canned response"))
	}
}
