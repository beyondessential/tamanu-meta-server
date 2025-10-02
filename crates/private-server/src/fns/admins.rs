use commons_errors::Result;
use leptos::server;

#[cfg(feature = "ssr")]
async fn guard() -> Result<database::Db> {
	use crate::state::AppState;
	use axum::extract::State;
	use commons_servers::tailscale_auth::TailscaleAdmin;
	use database::Db;
	use leptos::prelude::expect_context;
	use leptos_axum::extract_with_state;

	let state = expect_context::<AppState>();
	let TailscaleAdmin(_) = extract_with_state(&state).await?;
	let State(db): State<Db> = extract_with_state(&state).await?;
	Ok(db)
}

#[server]
pub async fn list() -> Result<Vec<String>> {
	let db = guard().await?;
	let mut conn = db.get().await?;

	database::admins::Admin::list(&mut conn)
		.await
		.map(|admins| admins.into_iter().map(|admin| admin.email).collect())
}

#[server]
pub async fn add(email: String) -> Result<()> {
	let db = guard().await?;
	let mut conn = db.get().await?;

	database::admins::Admin::add(&mut conn, &email).await?;
	Ok(())
}

#[server]
pub async fn delete(email: String) -> Result<()> {
	let db = guard().await?;
	let mut conn = db.get().await?;

	database::admins::Admin::delete(&mut conn, &email).await?;
	Ok(())
}
