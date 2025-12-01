use leptos::prelude::*;
use leptos_router::components::Outlet;

#[component]
pub fn SubTabs(
	/// The tab items. This should be a set of <a> or <A> elements, one for each tab.
	children: ChildrenFragment,
) -> impl IntoView {
	let children = children()
		.nodes
		.into_iter()
		.map(|child| child.attr("class", "navbar-item"))
		.collect::<Vec<_>>();

	view! {
		<div id="sub-tabs">
			<nav class="navbar is-fixed-bottom" role="navigation" aria-label="navigation">
				<div class="navbar-menu">
					<div class="navbar-start is-active">
						{children}
					</div>
				</div>
			</nav>
			<section><Outlet /></section>
		</div>
	}
}
