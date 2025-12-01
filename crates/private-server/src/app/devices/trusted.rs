use leptos::prelude::*;

use super::list::DeviceList;
use crate::components::PaginatedList;

#[component]
pub fn Trusted() -> impl IntoView {
	let (page, set_page) = signal(0i64);
	const PAGE_SIZE: i64 = 10;

	let total_count = Resource::new(|| (), async |_| crate::fns::devices::count_trusted().await);

	let trusted_devices = Resource::new(
		move || page.get(),
		|p| async move {
			let offset = p * PAGE_SIZE;
			crate::fns::devices::list_trusted(Some(PAGE_SIZE), Some(offset)).await
		},
	);

	view! {
		<div class="trusted-devices">
			<p class="section-description">
				"Devices that have been assigned a role and are trusted"
			</p>

			<Suspense fallback=|| view! { <div class="loading">"Loading devices..."</div> }>
				{move || trusted_devices.get().map(|result| {
					match result {
						Ok(devices) => {
							if devices.is_empty() {
								view! {
									<div class="no-devices">"No trusted devices found"</div>
								}.into_any()
							} else {
								view! {
									<PaginatedList
										page=page
										set_page=set_page
										total_count=Signal::derive(move || total_count.get().and_then(|r| r.ok()))
										page_size=PAGE_SIZE
									>
										<DeviceList devices=devices.clone() />
									</PaginatedList>
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
