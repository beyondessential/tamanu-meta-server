use leptos::prelude::*;

use super::DeviceList;

#[component]
pub fn Untrusted() -> impl IntoView {
	let (page, set_page) = signal(0i64);
	const PAGE_SIZE: i64 = 10;

	let total_count = Resource::new(
		|| (),
		async |_| crate::fns::devices::count_untrusted().await,
	);

	let untrusted_devices = Resource::new(
		move || page.get(),
		|p| async move {
			let offset = p * PAGE_SIZE;
			crate::fns::devices::list_untrusted(Some(PAGE_SIZE), Some(offset)).await
		},
	);

	view! {
		<div class="untrusted-devices">
			<p class="section-description">
				"Devices that have connected but haven't been assigned a role yet"
			</p>

			<Suspense fallback=|| view! { <div class="loading">"Loading devices..."</div> }>
				{move || untrusted_devices.get().map(|result| {
					match result {
						Ok(devices) => {
							if devices.is_empty() {
								view! {
									<div class="no-devices">"No untrusted devices found"</div>
								}.into_any()
							} else {
								view! {
									<div>
										<DeviceList devices=devices.clone() />
										<div class="pagination">
											<button
												class="pagination-btn"
												on:click=move |_| set_page.update(|p| *p = (*p).saturating_sub(1))
												disabled=move || page.get() == 0
											>
												"← Previous"
											</button>
											<span class="pagination-info">
												{move || {
													total_count.get().and_then(|r| r.ok()).map(|total| {
														let current_page = page.get() + 1;
														let total_pages = ((total as f64) / (PAGE_SIZE as f64)).ceil() as i64;
														format!("Page {} of {}", current_page, total_pages.max(1))
													}).unwrap_or_else(|| "Loading...".to_string())
												}}
											</span>
											<button
												class="pagination-btn"
												on:click=move |_| set_page.update(|p| *p += 1)
												disabled=move || {
													total_count.get()
														.and_then(|r| r.ok())
														.map(|total| (page.get() + 1) * PAGE_SIZE >= total)
														.unwrap_or(true)
												}
											>
												"Next →"
											</button>
										</div>
									</div>
								}.into_any()
							}
						}
						Err(e) => {
							view! {
								<div class="error">{format!("Error loading devices: {}", e)}</div>
							}.into_any()
						}
					}
				})}
			</Suspense>
		</div>
	}
}
