#[rocket::main]
async fn main() {
	tamanu_meta::server().await;
}
