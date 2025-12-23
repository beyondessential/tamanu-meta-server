use leptos::prelude::*;
use leptos_meta::Stylesheet;
use leptos_router::components::A;

use crate::components::SubTabs;

pub mod snippets;

#[component]
pub fn Page() -> impl IntoView {
	leptos_meta::provide_meta_context();

	view! {
		<Stylesheet id="css-bestool" href="/static/bestool.css" />
		<SubTabs>
			<A href="/bestool/snippets" exact=true>Snippets</A>
			<span>""</span>
		</SubTabs>
	}
}
