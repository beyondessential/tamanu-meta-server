use std::sync::Arc;

use leptos::prelude::*;
use leptos_router::components::A;

#[component]
pub fn DeviceList(devices: Vec<Arc<crate::fns::devices::DeviceInfo>>) -> impl IntoView {
	view! {
		<div class="device-list">
			<For each=move || devices.clone() key=|device| device.device.id let:device>
				<DeviceListItem device />
			</For>
		</div>
	}
}

#[component]
pub fn DeviceListItem(device: Arc<crate::fns::devices::DeviceInfo>) -> impl IntoView {
	let device_id = device.device.id.clone();
	let role = device.device.role;
	let latest_ip = device
		.latest_connection
		.as_ref()
		.map(|c| c.ip.clone())
		.unwrap_or_else(|| "—".to_string());
	let latest_user_agent = device
		.latest_connection
		.as_ref()
		.and_then(|c| c.user_agent.clone())
		.unwrap_or_else(|| "—".to_string());
	let first_seen = device.device.created_at_relative.clone();
	let first_seen_full = device.device.created_at.clone();
	let last_seen = device
		.latest_connection
		.as_ref()
		.map(|c| c.created_at_relative.clone())
		.unwrap_or_else(|| "Never".to_string());
	let last_seen_full = device
		.latest_connection
		.as_ref()
		.map(|c| c.created_at.clone())
		.unwrap_or_default();

	view! {
		<A href={format!("/devices/{device_id}")} {..} class="device-list-item">
			<div class="device-list-id">
				<span class="id-text">{device_id.to_string()}</span>
				<span class="role-badge">{role.to_string()}</span>
			</div>
			<div class="device-list-ip">{latest_ip}</div>
			<div class="device-list-ua">{latest_user_agent}</div>
			<div class="device-list-times">
				<span class="timestamp-hover" title={first_seen_full}>
					{format!("First seen: {}", first_seen)}
				</span>
				<span class="timestamp-hover" title={last_seen_full}>
					{format!("Last seen: {}", last_seen)}
				</span>
			</div>
		</A>
	}
}
