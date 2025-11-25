use leptos::prelude::*;
use leptos_meta::Stylesheet;
use leptos_router::hooks::use_params_map;

#[component]
pub fn Detail() -> impl IntoView {
	view! {
		<Stylesheet id="css-versions" href="/static/versions.css" />
		<div id="versions-page">
			<VersionDetail />
		</div>
	}
}

#[component]
fn VersionDetail() -> impl IntoView {
	let params = use_params_map();
	let version = move || params.read().get("version").unwrap_or_default();

	view! {
		<div class="page-header">
			<h1>{version}</h1>
		</div>
	}
}
