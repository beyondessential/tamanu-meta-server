use commons_errors::Result;
use leptos::server;

#[server]
pub async fn get_public_url() -> Result<Option<String>> {
	use std::env;

	Ok(env::var("PUBLIC_URL").ok())
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
