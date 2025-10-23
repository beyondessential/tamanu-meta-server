use leptos::prelude::*;

use crate::components::toast::ToastCtx;

use super::DeviceTable;

#[component]
pub fn Untrusted() -> impl IntoView {
	let ToastCtx(set_message) = use_context().unwrap();
	let (refresh_trigger, set_refresh_trigger) = signal(0);

	let untrusted_devices = Resource::new(
		move || refresh_trigger.get(),
		async |_| crate::fns::devices::list_untrusted().await,
	);

	let trust_device_action = Action::new(move |(device_id, role): &(String, String)| {
		let device_id = device_id.clone();
		let role = role.clone();
		async move { crate::fns::devices::trust(device_id, role).await }
	});

	Effect::new(move |_| {
		if let Some(result) = trust_device_action.value().get() {
			match result {
				Ok(_) => {
					set_message.set(Some("Device trusted successfully".to_string()));
					set_refresh_trigger.update(|n| *n += 1);

					set_timeout(
						move || set_message.set(None),
						std::time::Duration::from_millis(3000),
					);
				}
				Err(e) => {
					set_message.set(Some(format!("Error trusting device: {}", e)));
				}
			}
		}
	});

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
									<DeviceTable
										devices=devices.clone()
										trust_action=trust_device_action
									/>
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
