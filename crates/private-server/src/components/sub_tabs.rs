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
		.map(|child| child.attr("class", "tab-item"))
		.collect::<Vec<_>>();

	view! {
		<div id="sub-tabs">
			<nav>{children}</nav>
			<section><Outlet /></section>
		</div>
	}
}
