use leptos::prelude::*;
use leptos_meta::Stylesheet;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;
use uuid::Uuid;

use crate::components::{EndTabs, SubTabs};
use crate::fns::bestool::get_snippet;

pub mod snippets;

#[component]
pub fn Page() -> impl IntoView {
	leptos_meta::provide_meta_context();

	let params = use_params_map();
	let snippet_id = move || {
		params
			.read()
			.get("id")
			.and_then(|id| Uuid::parse_str(&id).ok())
	};

	let snippet_name = Resource::new(snippet_id, |id| async move {
		if let Some(id) = id {
			get_snippet(id).await.ok().map(|detail| (id, detail.name))
		} else {
			None
		}
	});

	view! {
		<Stylesheet id="css-bestool" href="/static/bestool.css" />
		<SubTabs>
			<A href="/bestool/snippets" exact=true>Snippets</A>
			<span>""</span>

			<EndTabs slot>
				<Transition>{move || {
					snippet_name.get().flatten().map(|(id, name)| {
						view! {
							<A href=format!("/bestool/snippets/{id}")>{name}</A>
						}
					})
				}}</Transition>
			</EndTabs>
		</SubTabs>
	}
}
