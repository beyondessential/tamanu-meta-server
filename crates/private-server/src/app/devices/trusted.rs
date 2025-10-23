use leptos::prelude::*;

use super::DeviceList;

#[component]
pub fn Trusted() -> impl IntoView {
	let trusted_devices = Resource::new(|| (), async |_| crate::fns::devices::list_trusted().await);

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
