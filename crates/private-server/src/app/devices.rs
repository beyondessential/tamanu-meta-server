use commons_types::Uuid;
use leptos::prelude::*;
use leptos_meta::{Stylesheet, provide_meta_context};
use leptos_router::components::A;
use std::collections::HashMap;

use crate::components::SubTabs;

mod detail;
mod search;
mod trusted;
mod untrusted;

pub use detail::Detail;
pub use search::Search;
pub use trusted::Trusted;
pub use untrusted::Untrusted;

#[derive(Debug, Clone)]
pub struct ConnectionGroup {
	ip: String,
	user_agent: Option<String>,
	count: usize,
	earliest_time: String,
	latest_time: String,
	earliest_relative: String,
	latest_relative: String,
}

fn group_consecutive_connections(
	connections: Vec<crate::fns::devices::DeviceConnectionData>,
) -> Vec<ConnectionGroup> {
	if connections.is_empty() {
		return vec![];
	}

	let mut groups = Vec::new();
	let mut current_group = vec![connections[0].clone()];

	for conn in connections.into_iter().skip(1) {
		let last_in_group = current_group.last().unwrap();

		if conn.ip == last_in_group.ip && conn.user_agent == last_in_group.user_agent {
			current_group.push(conn);
		} else {
			let group = create_group(current_group);
			groups.push(group);
			current_group = vec![conn];
		}
	}

	if !current_group.is_empty() {
		groups.push(create_group(current_group));
	}

	groups
}

fn create_group(connections: Vec<crate::fns::devices::DeviceConnectionData>) -> ConnectionGroup {
	let count = connections.len();
	let first = &connections[0];
	let last = connections.last().unwrap();

	ConnectionGroup {
		ip: first.ip.clone(),
		user_agent: first.user_agent.clone(),
		count,
		earliest_time: last.created_at.clone(),
		latest_time: first.created_at.clone(),
		earliest_relative: last.created_at_relative.clone(),
		latest_relative: first.created_at_relative.clone(),
	}
}

#[component]
pub fn Page() -> impl IntoView {
	provide_meta_context();
	let is_admin = Resource::new(
		|| (),
		|_| async { crate::fns::commons::is_current_user_admin().await },
	);

	view! {
		<Stylesheet id="css-devices" href="/static/devices.css" />
		<Suspense fallback=|| view! { <div class="loading">"Checking permissions..."</div> }>
			{move || {
				is_admin.get().map(|result| {
					match result {
						Ok(true) => {
							view! {
								<div id="devices-page">
									<div class="page-header">
										<h1>"Device Management"</h1>
										<p class="page-description">
											"Manage device approvals and trust levels."
										</p>
									</div>
									<SubTabs>
										<A href="" exact=true>Search</A>
										<A href="untrusted">Untrusted Devices</A>
										<A href="trusted">Trusted Devices</A>
									</SubTabs>
								</div>
							}.into_any()
						}
						Ok(false) => {
							view! {
								<div id="devices-page">
									<div class="page-header">
										<h1>"Access Denied"</h1>
									</div>
									<div class="error">
										<p>"You do not have permission to access device management."</p>
										<a href="/" class="back-link">"← Return to Home"</a>
									</div>
								</div>
							}.into_any()
						}
						Err(e) => {
							view! {
								<div id="devices-page">
									<div class="page-header">
										<h1>"Error"</h1>
									</div>
									<div class="error">
										{format!("Error checking permissions: {}", e)}
									</div>
								</div>
							}.into_any()
						}
					}
				})
			}}
		</Suspense>
	}
}

#[component]
pub fn DeviceList(devices: Vec<crate::fns::devices::DeviceInfo>) -> impl IntoView {
	view! {
		<div class="device-list">
			<For each=move || devices.clone() key=|device| device.device.id.clone() let:device>
				<DeviceListItem device=device />
			</For>
		</div>
	}
}

#[component]
pub fn DeviceListItem(device: crate::fns::devices::DeviceInfo) -> impl IntoView {
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

#[component]
pub fn DeviceConnectionHistory(device_id: Uuid) -> impl IntoView {
	let (history_offset, set_history_offset) = signal(0i64);
	let (all_connections, set_all_connections) =
		signal(HashMap::<Uuid, crate::fns::devices::DeviceConnectionData>::new());
	let (has_more, set_has_more) = signal(false);

	let connection_count = {
		let device_id = device_id.clone();
		Resource::new(
			move || device_id.clone(),
			async |id| crate::fns::devices::connection_count(id).await,
		)
	};

	let load_more_action = {
		let device_id = device_id.clone();
		Action::new(move |offset: &i64| {
			let device_id = device_id.clone();
			let offset = *offset;
			async move {
				crate::fns::devices::connection_history(device_id, Some(100), Some(offset)).await
			}
		})
	};

	Effect::new(move |_| {
		if let Some(result) = load_more_action.value().get() {
			match result {
				Ok(new_connections) => {
					let has_more_data = new_connections.len() == 100;
					set_has_more.set(has_more_data);

					set_all_connections.update(|existing| {
						for conn in new_connections {
							existing.insert(conn.id, conn);
						}
					});
				}
				Err(_) => {
					set_has_more.set(false);
				}
			}
		}
	});

	Effect::new(move |_| {
		if history_offset.get() == 0 && all_connections.get().is_empty() {
			load_more_action.dispatch(0);
		}
	});

	view! {
		<div class="connection-history">
			<h3>
				"Connection History"
				{move || {
					connection_count.get()
						.and_then(|result| result.ok())
						.map(|count| format!(" ({})", count))
						.unwrap_or_default()
				}}
			</h3>
			<Suspense fallback=|| view! { <div class="loading">"Loading history..."</div> }>
				{move || {
					let connections_map = all_connections.get();
					if connections_map.is_empty() && !load_more_action.pending().get() {
						view! {
							<div class="no-history">"No connection history found"</div>
						}.into_any()
					} else {
						let mut connections_vec: Vec<_> = connections_map.values().cloned().collect();
						connections_vec.sort_by(|a, b| b.created_at.cmp(&a.created_at));

						view! {
							<div class="history-content">
								<div class="history-list">
									<For each=move || group_consecutive_connections(connections_vec.clone()) key=|group| format!("{}_{}_{}", group.ip, group.earliest_time, group.latest_time) let:group>
										<ConnectionGroupRow group=group />
									</For>
								</div>
								{move || {
									if has_more.get() {
										view! {
											<div class="load-more-section">
												<button
													class="load-more-btn"
													on:click=move |_| {
														let current_count = all_connections.get().len() as i64;
														set_history_offset.set(current_count);
														load_more_action.dispatch(current_count);
													}
													disabled=move || load_more_action.pending().get()
												>
													{move || if load_more_action.pending().get() { "Loading..." } else { "Load More (100)" }}
												</button>
											</div>
										}.into_any()
									} else {
										().into_any()
									}
								}}
							</div>
						}.into_any()
					}
				}}
			</Suspense>
		</div>
	}
}

#[component]
pub fn ConnectionGroupRow(group: ConnectionGroup) -> impl IntoView {
	let time_display = if group.count == 1 {
		group.latest_relative.clone()
	} else {
		format!("{} to {}", group.earliest_relative, group.latest_relative)
	};

	let time_tooltip = if group.count == 1 {
		group.latest_time.clone()
	} else {
		format!("{} to {}", group.earliest_time, group.latest_time)
	};

	let count_display = if group.count > 1 {
		format!("{}×", group.count)
	} else {
		String::new()
	};

	view! {
		<div class="history-item">
			<div class="history-count">{count_display}</div>
			<div class="history-time timestamp-hover" title={time_tooltip}>{time_display}</div>
			<div class="history-ip">{group.ip}</div>
			{group.user_agent.as_ref().map(|ua| {
				view! {
					<div class="history-ua">{ua.clone()}</div>
				}
			})}
		</div>
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn create_test_connection(
		ip: &str,
		user_agent: Option<&str>,
		time: &str,
		relative: &str,
	) -> crate::fns::devices::DeviceConnectionData {
		crate::fns::devices::DeviceConnectionData {
			id: uuid::Uuid::new_v4().to_string(),
			created_at: time.to_string(),
			created_at_relative: relative.to_string(),
			device_id: uuid::Uuid::new_v4().to_string(),
			ip: ip.to_string(),
			user_agent: user_agent.map(|s| s.to_string()),
		}
	}

	#[test]
	fn test_group_consecutive_connections() {
		let connections = vec![
			create_test_connection(
				"192.168.1.1",
				Some("Agent1"),
				"2024-01-01T12:00:00Z",
				"1h ago",
			),
			create_test_connection(
				"192.168.1.1",
				Some("Agent1"),
				"2024-01-01T11:00:00Z",
				"2h ago",
			),
			create_test_connection(
				"192.168.1.2",
				Some("Agent2"),
				"2024-01-01T10:00:00Z",
				"3h ago",
			),
		];

		let groups = group_consecutive_connections(connections);

		assert_eq!(groups.len(), 2);
		assert_eq!(groups[0].count, 2);
		assert_eq!(groups[0].ip, "192.168.1.1");
		assert_eq!(groups[1].count, 1);
		assert_eq!(groups[1].ip, "192.168.1.2");
	}

	#[test]
	fn test_group_different_user_agents() {
		let connections = vec![
			create_test_connection(
				"192.168.1.1",
				Some("Agent1"),
				"2024-01-01T12:00:00Z",
				"1h ago",
			),
			create_test_connection(
				"192.168.1.1",
				Some("Agent2"),
				"2024-01-01T11:00:00Z",
				"2h ago",
			),
		];

		let groups = group_consecutive_connections(connections);
		assert_eq!(groups.len(), 2);
	}

	#[test]
	fn test_group_empty_connections() {
		let groups = group_consecutive_connections(vec![]);
		assert_eq!(groups.len(), 0);
	}

	#[test]
	fn test_hashmap_deduplication() {
		let mut map = HashMap::new();
		let conn1 = create_test_connection(
			"192.168.1.1",
			Some("Agent1"),
			"2024-01-01T12:00:00Z",
			"1h ago",
		);
		let conn2 = create_test_connection(
			"192.168.1.1",
			Some("Agent1"),
			"2024-01-01T11:00:00Z",
			"2h ago",
		);

		let id1 = conn1.id.clone();
		let id2 = conn2.id.clone();

		map.insert(id1.clone(), conn1);
		map.insert(id2.clone(), conn2);

		assert_eq!(map.len(), 2);

		let duplicate = create_test_connection(
			"192.168.1.1",
			Some("Agent1"),
			"2024-01-01T12:00:00Z",
			"1h ago",
		);
		map.insert(id1.clone(), duplicate);

		assert_eq!(map.len(), 2);
	}
}
