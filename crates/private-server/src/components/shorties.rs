use std::sync::Arc;

use leptos::prelude::*;
use leptos_router::components::A;

use crate::{
	components::{DeviceRoleBadge, ServerKindBadge, ServerRankBadge},
	fns::{devices::DeviceInfo, servers::ServerInfo},
};

#[component]
pub fn DeviceShorty(device: Arc<DeviceInfo>) -> impl IntoView {
	let id = device.device.id;
	let role = device.device.role;
	let name = device.name();

	view! {
		<div class="level">
			<div class="level-left">
				<A href={format!("/devices/{}", id)} {..} class="level-item">
					{name}
				</A>
			</div>
			<div class="level-right"><DeviceRoleBadge role /></div>
		</div>
	}
}

#[component]
pub fn ServerShorty(server: Arc<ServerInfo>) -> impl IntoView {
	let id = server.id;
	let rank = server.rank;
	let kind = server.kind;
	let host = server.host.clone();
	let name = server
		.name
		.clone()
		.unwrap_or_else(|| "Unnamed server".to_string());

	view! {
		<div class="level">
			<div class="level-left">
				<A href={format!("/servers/{}", id)} {..} class="level-item">
					{name}
				</A>
				{rank.map(|rank| view! { <ServerRankBadge rank /> })}
				<ServerKindBadge kind />
			</div>
			<div class="level-right">
				<span class="level-item">{host.clone()}</span>
			</div>
		</div>
	}
}
