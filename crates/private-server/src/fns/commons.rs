use commons_errors::Result;
use leptos::server;

#[server]
pub async fn public_url() -> Result<Option<String>> {
	use std::env;

	Ok(env::var("PUBLIC_URL").ok())
}

#[server]
pub async fn server_versions_url() -> Result<Option<String>> {
	use std::env;

	Ok((|| {
		let public_url = env::var("PUBLIC_URL").ok()?;
		let secret = env::var("SERVER_VERSIONS_SECRET").ok()?;
		Some(format!("{public_url}/server-versions?s={secret}"))
	})())
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

#[cfg(feature = "ssr")]
pub async fn admin_guard() -> Result<database::Db> {
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
