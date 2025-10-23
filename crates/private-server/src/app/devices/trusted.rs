use leptos::prelude::*;

use crate::components::toast::ToastCtx;

use super::DeviceTable;

#[component]
pub fn Trusted() -> impl IntoView {
	let ToastCtx(set_message) = use_context().unwrap();
	let (refresh_trigger, set_refresh_trigger) = signal(0);

	let update_role_action = Action::new(move |(device_id, role): &(String, String)| {
		let device_id = device_id.clone();
		let role = role.clone();
		async move { crate::fns::devices::update_role(device_id, role).await }
	});

	Effect::new(move |_| {
		if let Some(result) = update_role_action.value().get() {
			match result {
				Ok(_) => {
					set_message.set(Some("Device role updated successfully".to_string()));

					set_timeout(
						move || set_message.set(None),
						std::time::Duration::from_millis(3000),
					);
				}
				Err(e) => {
					set_message.set(Some(format!("Error updating device role: {}", e)));
				}
			}
		}
	});

	let trusted_devices = Resource::new(
		move || refresh_trigger.get(),
		async |_| crate::fns::devices::list_trusted().await,
	);

	let untrust_device_action = Action::new(move |device_id: &String| {
		let device_id = device_id.clone();
		async move { crate::fns::devices::untrust(device_id).await }
	});

	Effect::new(move |_| {
		if let Some(result) = untrust_device_action.value().get() {
			match result {
				Ok(_) => {
					set_message.set(Some("Device untrusted successfully".to_string()));
					set_refresh_trigger.update(|n| *n += 1);

					set_timeout(
						move || set_message.set(None),
						std::time::Duration::from_millis(3000),
					);
				}
				Err(e) => {
					set_message.set(Some(format!("Error untrusting device: {}", e)));
				}
			}
		}
	});

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
									<DeviceTable
										devices=devices.clone()
										untrust_action=untrust_device_action
										update_role_action=update_role_action
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
