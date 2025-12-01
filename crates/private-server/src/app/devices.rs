use leptos::prelude::*;
use leptos_meta::{Stylesheet, provide_meta_context};
use leptos_router::{components::A, hooks::use_params_map};

use crate::components::{EndTabs, SubTabs};

mod detail;
mod history;
pub mod list;
mod search;
mod trusted;
mod untrusted;

pub use detail::Detail;
pub use search::Search;
pub use trusted::Trusted;
pub use untrusted::Untrusted;

#[component]
pub fn Page() -> impl IntoView {
	provide_meta_context();

	let params = use_params_map();
	let device_id = move || params.read().get("id");

	view! {
		<Stylesheet id="css-devices" href="/static/devices.css" />
		<SubTabs>
			<A href="" exact=true>Search</A>
			<A href="untrusted">Untrusted Devices</A>
			<A href="trusted">Trusted Devices</A>

			<EndTabs slot>
			{move || {
				device_id().map(|id| {
					view! {
						<A href=format!("/devices/{id}")>{id.to_string()}</A>
					}
				})
			}}
			</EndTabs>
		</SubTabs>
	}
}
