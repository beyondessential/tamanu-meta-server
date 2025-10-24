use leptos::prelude::*;
use leptos_meta::{Stylesheet, provide_meta_context};
use leptos_router::components::A;

use crate::components::SubTabs;

mod centrals;
mod detail;
mod edit;
mod facilities;

pub use centrals::Centrals;
pub use detail::Detail;
pub use edit::Edit;
pub use facilities::Facilities;

#[component]
pub fn Page() -> impl IntoView {
	provide_meta_context();

	view! {
		<Stylesheet id="css-servers" href="/static/servers.css" />
		<div id="servers-page">
			<div class="page-header">
				<h1>"Servers"</h1>
				<p class="page-description">
					"Manage central and facility servers."
				</p>
			</div>
			<SubTabs>
				<A href="" exact=true>Central Servers</A>
				<A href="facilities">Facility Servers</A>
			</SubTabs>
		</div>
	}
}
