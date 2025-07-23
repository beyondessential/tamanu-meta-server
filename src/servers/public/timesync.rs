use timesimp::{Request, SignedDuration, Timesimp};

use crate::{
	error::{AppError, Result},
	servers::headers::TamanuHeaders,
};

#[post("/timesync", data = "<request>")]
pub async fn endpoint(request: Vec<u8>) -> Result<TamanuHeaders<Vec<u8>>> {
	let response = ServerSimp
		.answer_client(Request::try_from(request.as_ref())?)
		.await?;
	Ok(TamanuHeaders::new(response.to_bytes().to_vec()))
}

struct ServerSimp;
impl Timesimp for ServerSimp {
	type Err = AppError;

	async fn load_offset(&self) -> Result<Option<SignedDuration>, Self::Err> {
		// server time is correct
		Ok(Some(SignedDuration::ZERO))
	}

	async fn store_offset(&mut self, _offset: SignedDuration) -> Result<(), Self::Err> {
		// as time is correct, no need to store offset
		unimplemented!()
	}

	async fn query_server(
		&self,
		_request: timesimp::Request,
	) -> Result<timesimp::Response, Self::Err> {
		// server has no upstream timesimp
		unimplemented!()
	}

	async fn sleep(duration: std::time::Duration) {
		rocket::tokio::time::sleep(duration).await;
	}
}
