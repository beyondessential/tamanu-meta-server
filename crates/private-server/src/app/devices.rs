use std::str::FromStr as _;

use leptos::prelude::*;
use leptos_meta::{Stylesheet, provide_meta_context};
use leptos_router::{components::A, hooks::use_params_map};
use uuid::Uuid;

use crate::{
	components::{EndTabs, SubTabs},
	fns::devices::get_device_name_by_id,
};

mod detail;
mod history;
pub mod list;
mod search;

pub use detail::Detail;
pub use search::Search;

#[component]
pub fn Page() -> impl IntoView {
	provide_meta_context();

	let params = use_params_map();
	let device_id = move || {
		params
			.read()
			.get("id")
			.and_then(|id| Uuid::from_str(&id).ok())
	};
	let device_name = Resource::new(
		move || device_id(),
		async move |id| {
			if let Some(id) = id {
				get_device_name_by_id(id)
					.await
					.ok()
					.and_then(|name| Some((id, name)))
			} else {
				None
			}
		},
	);

	view! {
		<Stylesheet id="css-devices" href="/static/devices.css" />
		<SubTabs>
			<A href="" exact=true>Search</A>
			<A href="untrusted">Untrusted Devices</A>
			<A href="trusted">Trusted Devices</A>

			<EndTabs slot>
				<Transition>{move || {
					device_name.get().flatten().map(|(id, name)| {
						view! {
							<A href=format!("/devices/{id}")>{name}</A>
						}
					})
				}}</Transition>
			</EndTabs>
		</SubTabs>
	}
}
