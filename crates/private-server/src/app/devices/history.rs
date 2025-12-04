use std::collections::HashMap;

use commons_types::Uuid;
use jiff::{RoundMode, SignedDurationRound, Unit};
use leptos::prelude::*;

use crate::{components::TimeAgo, fns::devices::DeviceConnectionData};

pub fn connection_count(device_id: Uuid) -> LocalResource<u64> {
	LocalResource::new(move || async move {
		crate::fns::devices::connection_count(device_id)
			.await
			.unwrap_or_default()
	})
}

#[component]
pub fn ConnectionHistory(device_id: Uuid) -> impl IntoView {
	let (show_history, set_show_history) = signal(false);

	let count = connection_count(device_id);

	view! {
		<div class="level">
			<button
				class="level-item button"
				on:click=move |_| set_show_history.update(|show| *show = !*show)
			>
				{move || {
					if show_history.get() {
						"Hide connection history"
					} else {
						"Show connection history"
					}
				}}
				" "
				<Transition>
					{move || count.get().map(|n| format!("({n})"))}
				</Transition>
			</button>
		</div>
		{move || {
			show_history.get().then(|| {
				view! {
					<DeviceConnectionHistory device_id />
				}
			})
		}}
	}
}

const BATCH: usize = 1000;

#[component]
fn DeviceConnectionHistory(device_id: Uuid) -> impl IntoView {
	let (history_offset, set_history_offset) = signal(0u64);
	let (all_connections, set_all_connections) =
		signal(HashMap::<Uuid, DeviceConnectionData>::new());
	let (has_more, set_has_more) = signal(false);

	let load_more_action = {
		Action::new(move |offset: &u64| {
			let device_id = device_id;
			let offset = *offset;
			async move {
				crate::fns::devices::connection_history(device_id, Some(BATCH as _), Some(offset))
					.await
			}
		})
	};

	Effect::new(move |_| {
		if let Some(result) = load_more_action.value().get() {
			match result {
				Ok(new_connections) => {
					let has_more_data = new_connections.len() == BATCH;
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
		<Transition fallback=|| view! { <div class="box"><progress class="progress is-small is-primary" max="100">"Loading..."</progress></div> }>
			{move || {
				let connections_map = all_connections.get();
				if connections_map.is_empty() && !load_more_action.pending().get() {
					return view! {
						<p>"No connection history found"</p>
					}.into_any();
				}

				let mut connections_vec: Vec<_> = connections_map.values().cloned().collect();
				connections_vec.sort_by(|a, b| b.created_at.cmp(&a.created_at));

				view! {
					<div class="box">
						<For
							each=move || group_consecutive_connections(connections_vec.clone())
							key=|group| (group.ip.clone(), group.earliest_time, group.latest_time)
							let:group
						>
							<ConnectionGroupRow group />
						</For>
					</div>
					{move || {
						has_more.get().then(|| {
							view! {
								<div class="level">
									<button
										class="level-item button"
										on:click=move |_| {
											let current_count = all_connections.get().len() as u64;
											set_history_offset.set(current_count);
											load_more_action.dispatch(current_count);
										}
										disabled=move || load_more_action.pending().get()
									>
										{move || if load_more_action.pending().get() { "Loading...".into() } else { format!("Load More ({BATCH})") }}
									</button>
								</div>
							}
						})
					}}
				}.into_any()
			}}
		</Transition>
	}
}

#[component]
pub fn ConnectionGroupRow(group: ConnectionGroup) -> impl IntoView {
	let span = group.latest_time.duration_since(group.earliest_time);
	let span = span
		.round(
			SignedDurationRound::new()
				.smallest(Unit::Second)
				.increment(30)
				.mode(RoundMode::Ceil),
		)
		.unwrap_or(span);
	view! {
		<div class="level">
			<div class="level-left">
				<div class="level-item history-times">
					<TimeAgo timestamp={group.earliest_time} />
					<span>" to "</span>
					<TimeAgo timestamp={group.latest_time} />
				</div>
				<div class="level-item">
					{format!("{:#}", span)}
				</div>
				<div class="level-item monospace">{group.ip}</div>
				{(group.count > 1).then(|| view! {
					<div class="level-item history-count">{group.count}"×"</div>
				})}
			</div>
			<div class="level-right">
				{group.user_agent.as_ref().map(|ua| {
					view! {
						<div class="level-item">{ua.clone()}</div>
					}
				})}
			</div>
		</div>
	}
}

#[derive(Debug, Clone)]
pub struct ConnectionGroup {
	ip: String,
	user_agent: Option<String>,
	count: usize,
	earliest_time: jiff::Timestamp,
	latest_time: jiff::Timestamp,
}

fn group_consecutive_connections(connections: Vec<DeviceConnectionData>) -> Vec<ConnectionGroup> {
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

fn create_group(connections: Vec<DeviceConnectionData>) -> ConnectionGroup {
	let count = connections.len();
	let first = connections.first().unwrap();
	let last = connections.last().unwrap();

	ConnectionGroup {
		ip: first.ip.clone(),
		user_agent: first.user_agent.clone(),
		count,
		earliest_time: last.created_at,
		latest_time: first.created_at,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn create_test_connection(
		ip: &str,
		user_agent: Option<&str>,
		time: &str,
	) -> crate::fns::devices::DeviceConnectionData {
		crate::fns::devices::DeviceConnectionData {
			id: uuid::Uuid::new_v4(),
			created_at: time.parse().unwrap(),
			device_id: uuid::Uuid::new_v4(),
			ip: ip.to_string(),
			user_agent: user_agent.map(|s| s.to_string()),
		}
	}

	#[test]
	fn test_group_consecutive_connections() {
		let connections = vec![
			create_test_connection("192.168.1.1", Some("Agent1"), "2024-01-01T12:00:00Z"),
			create_test_connection("192.168.1.1", Some("Agent1"), "2024-01-01T11:00:00Z"),
			create_test_connection("192.168.1.2", Some("Agent2"), "2024-01-01T10:00:00Z"),
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
			create_test_connection("192.168.1.1", Some("Agent1"), "2024-01-01T12:00:00Z"),
			create_test_connection("192.168.1.1", Some("Agent2"), "2024-01-01T11:00:00Z"),
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
		let conn1 = create_test_connection("192.168.1.1", Some("Agent1"), "2024-01-01T12:00:00Z");
		let conn2 = create_test_connection("192.168.1.1", Some("Agent1"), "2024-01-01T11:00:00Z");

		let id1 = conn1.id;
		let id2 = conn2.id;

		map.insert(id1, conn1);
		map.insert(id2, conn2);

		assert_eq!(map.len(), 2);

		let duplicate =
			create_test_connection("192.168.1.1", Some("Agent1"), "2024-01-01T12:00:00Z");
		map.insert(id1, duplicate);

		assert_eq!(map.len(), 2);
	}
}
