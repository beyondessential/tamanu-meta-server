use std::sync::Arc;

use commons_types::{
	device::DeviceRole,
	server::{kind::ServerKind, rank::ServerRank},
};
use leptos::prelude::*;
use leptos_router::components::A;

use crate::fns::{devices::DeviceInfo, servers::ServerInfo};

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
			<div class="level-right">
				<span class={format!("level-item tag is-capitalized {}", match role {
					DeviceRole::Untrusted => "is-danger",
					DeviceRole::Server => "is-primary",
					DeviceRole::Releaser => "is-warning",
					DeviceRole::Admin => "is-info",
				})}>{role}</span>
			</div>
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
				{rank.map(|rank| {
					view! {
						<span class={format!("level-item tag is-capitalized {}", match rank {
							ServerRank::Production => "is-danger",
							ServerRank::Clone => "is-warning",
							ServerRank::Demo => "is-link",
							ServerRank::Test => "is-info",
							ServerRank::Dev => "is-success",
						})}>{rank}</span>
					}
				})}
				<span class={format!("level-item tag is-capitalized {}", match kind {
					ServerKind::Central => "is-link",
					ServerKind::Facility => "is-info",
					ServerKind::Meta => ""
				})}>{kind}</span>
			</div>
			<div class="level-right">
				<span class="level-item">{host.clone()}</span>
			</div>
		</div>
	}
}
