#[rocket::main]
async fn main() {
	tamanu_meta::private_server().await;
}
