use axum::{
	body::Bytes,
	routing::{Router, post},
};
use timesimp::{Request, SignedDuration, Timesimp};

use crate::{
	error::{AppError, Result},
	state::AppState,
};

pub fn routes() -> Router<AppState> {
	Router::new().route("/timesync", post(endpoint))
}

async fn endpoint(request: Bytes) -> Result<Vec<u8>> {
	let payload = request.as_ref();
	if payload.len() != 8 {
		return Err(AppError::custom("payload size mismatch"));
	}

	let response = ServerSimp
		.answer_client(Request::try_from(payload)?)
		.await?;
	Ok(response.to_bytes().to_vec())
}

struct ServerSimp;
impl Timesimp for ServerSimp {
	type Err = AppError;

	async fn load_offset(&self) -> Result<Option<SignedDuration>> {
		// server time is correct
		Ok(Some(SignedDuration::ZERO))
	}

	async fn store_offset(&mut self, _offset: SignedDuration) -> Result<()> {
		// as time is correct, no need to store offset
		unimplemented!()
	}

	async fn query_server(&self, _request: timesimp::Request) -> Result<timesimp::Response> {
		// server has no upstream timesimp
		unimplemented!()
	}

	async fn sleep(duration: std::time::Duration) {
		tokio::time::sleep(duration).await;
	}
}
