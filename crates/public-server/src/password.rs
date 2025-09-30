use std::sync::Arc;

use axum::{
	extract::State,
	response::Html,
	routing::{Router, get},
};
use tera::{Context, Tera};

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
	Router::new().route("/password", get(view))
}

async fn view(State(tera): State<Arc<Tera>>) -> Html<String> {
	Html(tera.render("password", &Context::new()).unwrap())
}
