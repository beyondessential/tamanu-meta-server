use leptos::prelude::*;
use leptos_meta::{Stylesheet, provide_meta_context};
use leptos_router::{components::A, hooks::use_params_map};

use crate::components::{EndTabs, SubTabs};

mod detail;
// mod edit;
pub mod list;

pub use detail::Detail;
// pub use edit::Edit;

#[component]
pub fn Page() -> impl IntoView {
	provide_meta_context();

	let params = use_params_map();
	let server_id = move || params.read().get("id");

	view! {
		<Stylesheet id="css-servers" href="/static/servers.css" />
		<section class="section" id="servers-page">
			<SubTabs>
				<A href="" exact=true>Central Servers</A>
				<A href="facilities">Facility Servers</A>

				<EndTabs slot>
				{move || {
					server_id().map(|id| {
						view! {
							<A href=format!("/servers/{id}")>{id.to_string()}</A>
						}
					})
				}}
				</EndTabs>
			</SubTabs>
		</section>
	}
}
