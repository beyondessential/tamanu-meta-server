use rocket_db_pools::diesel::SimpleAsyncConnection;

#[rocket::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	let db = tamanu_meta::db_pool().await?;
	let mut conn = db.get().await?;
	conn.batch_execute("SELECT prune_untrusted_devices();").await?;
	Ok(())
}
