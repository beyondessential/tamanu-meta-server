use leptos::prelude::*;

use super::list::DeviceList;

#[component]
pub fn Search() -> impl IntoView {
	let (search_query, set_search_query) = signal(String::new());

	let search_results = Resource::new(
		move || search_query.get(),
		async |query| {
			if query.trim().is_empty() {
				Ok(vec![])
			} else {
				crate::fns::devices::search(query).await
			}
		},
	);

	view! {
		<div class="box mt-3">
			<h2 class="is-size-3">"Search by public key"</h2>
			<div class="field">
				<div class="control">
					<input
						type="search"
						placeholder="Paste PEM key fragment…"
						prop:value=move || search_query.get()
						on:input=move |ev| set_search_query.set(event_target_value(&ev))
						class="input"
					/>
				</div>
			</div>
		</div>

		<Suspense fallback=|| view! { <progress class="progress is-small is-primary" max="100">"Loading..."</progress> }>
			{move || {
				let query = search_query.get();
				(!query.trim().is_empty()).then_some(()).and(search_results.get()).map(|result| {
					match result {
						Ok(devices) => {
							if devices.is_empty() {
								view! {
									<div class="box has-info-text">"No devices found matching your search"</div>
								}.into_any()
							} else {
								view! {
									<DeviceList devices />
								}.into_any()
							}
						}
						Err(e) => {
							view! {
								<div class="box has-danger-text">{format!("Search error: {}", e)}</div>
							}.into_any()
						}
					}
				})
			}}
		</Suspense>
	}
}
