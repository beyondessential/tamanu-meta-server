use std::str::FromStr as _;

use leptos::prelude::*;
use leptos_meta::{Stylesheet, provide_meta_context};
use leptos_router::{components::A, hooks::use_params_map};
use uuid::Uuid;

use crate::{
	components::{EndTabs, SubTabs},
	fns::servers::get_name,
};

mod detail;
// mod edit;
pub mod list;

pub use detail::Detail;
// pub use edit::Edit;

#[component]
pub fn Page() -> impl IntoView {
	provide_meta_context();

	let params = use_params_map();
	let server_id = move || {
		params
			.read()
			.get("id")
			.and_then(|id| Uuid::from_str(&id).ok())
	};
	let server_name = Resource::new(
		move || server_id(),
		async move |id| {
			if let Some(id) = id {
				get_name(id).await.ok().and_then(|name| Some((id, name)))
			} else {
				None
			}
		},
	);

	view! {
		<Stylesheet id="css-servers" href="/static/servers.css" />
		<section class="section" id="servers-page">
			<SubTabs>
				<A href="" exact=true>Central Servers</A>
				<A href="facilities">Facility Servers</A>

				<EndTabs slot>
					<Transition>{move || {
						server_name.get().flatten().map(|(id, name)| {
							view! {
								<A href=format!("/servers/{id}")>{name}</A>
							}
						})
					}}</Transition>
				</EndTabs>
			</SubTabs>
		</section>
	}
}
