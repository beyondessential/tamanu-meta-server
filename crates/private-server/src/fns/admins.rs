use commons_errors::Result;
use leptos::server;

#[server]
pub async fn list() -> Result<Vec<String>> {
	let db = crate::fns::commons::admin_guard().await?;
	let mut conn = db.get().await?;

	database::admins::Admin::list(&mut conn)
		.await
		.map(|admins| admins.into_iter().map(|admin| admin.email).collect())
}

#[server]
pub async fn add(email: String) -> Result<()> {
	let db = crate::fns::commons::admin_guard().await?;
	let mut conn = db.get().await?;

	database::admins::Admin::add(&mut conn, &email).await?;
	Ok(())
}

#[server]
pub async fn delete(email: String) -> Result<()> {
	let db = crate::fns::commons::admin_guard().await?;
	let mut conn = db.get().await?;

	database::admins::Admin::delete(&mut conn, &email).await?;
	Ok(())
}
