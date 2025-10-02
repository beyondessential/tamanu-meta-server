use leptos::prelude::*;

#[component]
pub fn Greeting() -> impl IntoView {
	let greeting = crate::fns::statuses::greeting();

	view! {
		<Await future=greeting let:data>
			<div class="greeting">{data.clone().ok()}</div>
		</Await>
	}
}
