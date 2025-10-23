use leptos::prelude::*;

use super::DeviceList;

#[component]
pub fn Untrusted() -> impl IntoView {
	let untrusted_devices =
		Resource::new(|| (), async |_| crate::fns::devices::list_untrusted().await);

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
									<DeviceList devices=devices.clone() />
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
