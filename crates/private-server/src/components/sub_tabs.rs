use leptos::{prelude::*, tachys::view::fragment::IntoFragment as _};
use leptos_router::components::Outlet;

use crate::components::ToggleSignal as _;

#[derive(Default)]
#[slot]
pub struct EndTabs {
	#[prop(optional)]
	children: Option<Children>,
}

#[component]
pub fn SubTabs(
	/// The tab items that go at the end.
	#[prop(optional)]
	end_tabs: EndTabs,

	/// The tab items. This should be a set of <a> or <A> elements, one for each tab.
	children: ChildrenFragment,
) -> impl IntoView {
	let start_children = children()
		.nodes
		.into_iter()
		.map(|child| child.attr("class", "navbar-item"))
		.collect::<Vec<_>>();

	let end_children = end_tabs.children.map(|children| {
		children()
			.into_fragment()
			.nodes
			.into_iter()
			.map(|child| child.attr("class", "navbar-item"))
			.collect::<Vec<_>>()
	});

	let (burgered, set_burgered) = signal(false);

	view! {
		<div id="sub-tabs">
			<nav class="navbar is-fixed-bottom" role="navigation" aria-label="navigation">
				<div class="navbar-menu" class:is-active={move || burgered.get()}>
					<div class="navbar-start is-active">
						{start_children}
					</div>
					{end_children.map(|children| view! {
						<div class="navbar-end">
							{children}
						</div>
					})}
				</div>
				<div class="navbar-brand">
					<a
						class="navbar-burger"
						role="button"
						aria-label="menu"
						aria-expanded=move || burgered.get().to_string()
						class:is-active=move || burgered.get()
						on:click=move |_| set_burgered.toggle()
					>
						<span aria-hidden="true"></span>
						<span aria-hidden="true"></span>
						<span aria-hidden="true"></span>
						<span aria-hidden="true"></span>
					</a>
				</div>
			</nav>
			<section><Outlet /></section>
		</div>
	}
}
