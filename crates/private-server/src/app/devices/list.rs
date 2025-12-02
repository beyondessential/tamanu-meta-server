use std::sync::Arc;

use leptos::prelude::*;
use leptos_router::components::A;

use crate::components::TimeAgo;

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
	let first_seen = device.device.created_at;
	let last_seen = device
		.latest_connection
		.as_ref()
		.map(|c| c.created_at)
		.unwrap_or_default();

	view! {
		<A href={format!("/devices/{device_id}")} {..} class="device-list-item">
			<div class="device-list-id">
				<span class="id-text">{device_id.to_string()}</span>
				<span class="role-badge">{role}</span>
			</div>
			<div class="device-list-ip">{latest_ip}</div>
			<div class="device-list-ua">{latest_user_agent}</div>
			<div class="info-item">
				<span class="info-label">"First seen"</span>
				<TimeAgo timestamp={first_seen} {..} class:info-value />
			</div>
			<div class="info-item">
				<span class="info-label">"Last seen"</span>
				<TimeAgo timestamp={last_seen} {..} class:info-value />
			</div>
		</A>
	}
}
