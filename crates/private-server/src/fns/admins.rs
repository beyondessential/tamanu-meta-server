use commons_errors::Result;
use leptos::server;

#[server]
pub async fn list() -> Result<Vec<String>> {
	use crate::state::AppState;
	use axum::extract::State;
	use commons_servers::tailscale_auth::TailscaleAdmin;
	use database::Db;
	use database::admins::Admin;
	use leptos::prelude::expect_context;
	use leptos_axum::extract_with_state;

	let state = expect_context::<AppState>();
	let TailscaleAdmin(_) = extract_with_state(&state).await?;
	let State(db): State<Db> = extract_with_state(&state).await?;
	let mut conn = db.get().await?;

	Admin::list(&mut conn)
		.await
		.map(|admins| admins.into_iter().map(|admin| admin.email).collect())
}

#[server]
pub async fn add_admin(email: String) -> Result<()> {
	use crate::state::AppState;
	use axum::extract::State;
	use commons_servers::tailscale_auth::TailscaleAdmin;
	use database::Db;
	use database::admins::Admin;
	use leptos::prelude::expect_context;
	use leptos_axum::extract_with_state;

	let state = expect_context::<AppState>();
	let TailscaleAdmin(_) = extract_with_state(&state).await?;
	let State(db): State<Db> = extract_with_state(&state).await?;
	let mut conn = db.get().await?;

	Admin::add(&mut conn, &email).await?;
	Ok(())
}

#[server]
pub async fn delete_admin(email: String) -> Result<()> {
	use crate::state::AppState;
	use axum::extract::State;
	use commons_servers::tailscale_auth::TailscaleAdmin;
	use database::Db;
	use database::admins::Admin;
	use leptos::prelude::expect_context;
	use leptos_axum::extract_with_state;

	let state = expect_context::<AppState>();
	let TailscaleAdmin(_) = extract_with_state(&state).await?;
	let State(db): State<Db> = extract_with_state(&state).await?;
	let mut conn = db.get().await?;

	Admin::delete(&mut conn, &email).await?;
	Ok(())
}

#[server]
pub async fn is_current_user_admin() -> Result<bool> {
	use crate::state::AppState;
	use commons_servers::tailscale_auth::TailscaleAdmin;
	use leptos::prelude::expect_context;
	use leptos_axum::extract_with_state;

	let state = expect_context::<AppState>();
	let TailscaleAdmin(_) = extract_with_state(&state).await?;

	Ok(true)
}
