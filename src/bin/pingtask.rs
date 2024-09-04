#[rocket::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	let db = tamanu_meta::db_pool().await?;
	tamanu_meta::pingtask::spawn(db).await?;
	Ok(())
}
