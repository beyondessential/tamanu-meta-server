use leptos::prelude::*;

#[component]
pub fn Page() -> impl IntoView {
	view! {
		<header class="header">Hi</header>
		<List />
	}
}

#[island]
pub fn List() -> impl IntoView {
	let (r, w) = signal(0);
	Effect::new(move |_| w.set(1));
	let list = Resource::new(move || r.get(), async |_| crate::fns::admins::list().await);

	view! {
		<Suspense fallback=|| view! { <div class="loading">"Loading…"</div> }>
			{move || list.get().map(|data| view! {
				<ul>
				<For each=move || data.clone() key=|a| a.clone() let(a)><li>{a}</li></For>
				</ul>
			})}
		</Suspense>
	}
}
