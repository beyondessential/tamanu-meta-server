#[rocket::main]
async fn main() {
	tamanu_meta::public_server().await;
}
