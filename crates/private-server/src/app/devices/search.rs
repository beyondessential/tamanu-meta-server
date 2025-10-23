use leptos::prelude::*;

use super::DeviceList;

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
		<div class="device-search">
			<h2>"Search Devices by Key"</h2>
			<div class="search-box">
				<input
					type="text"
					placeholder="Paste PEM or hex key fragment..."
					prop:value=move || search_query.get()
					on:input=move |ev| set_search_query.set(event_target_value(&ev))
					class="search-input"
				/>
				<p class="search-help">
					"Search by pasting a key fragment in PEM format or hex (with or without colons)"
				</p>
			</div>

			<Suspense fallback=|| view! { <div class="loading">"Searching..."</div> }>
				{move || {
					let query = search_query.get();
					if query.trim().is_empty() {
						().into_any()
					} else {
						search_results.get().map(|result| {
							match result {
								Ok(devices) => {
									if devices.is_empty() {
										view! {
											<div class="no-results">"No devices found matching your search"</div>
										}.into_any()
									} else {
										view! {
											<div class="search-results">
												<h3>{format!("Search Results ({} found)", devices.len())}</h3>
												<DeviceList devices=devices />
											</div>
										}.into_any()
									}
								}
								Err(e) => {
									view! {
										<div class="error">{format!("Search error: {}", e)}</div>
									}.into_any()
								}
							}
						}).unwrap_or_else(|| ().into_any())
					}
				}}
			</Suspense>
		</div>
	}
}
