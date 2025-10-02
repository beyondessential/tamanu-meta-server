use leptos::prelude::*;
use leptos_meta::{Stylesheet, provide_meta_context};
use std::collections::HashMap;
use web_sys::window;

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
		earliest_time: last.created_at.clone(), // last in list is earliest chronologically
		latest_time: first.created_at.clone(),  // first in list is latest chronologically
		earliest_relative: last.created_at_relative.clone(),
		latest_relative: first.created_at_relative.clone(),
	}
}

#[island]
pub fn Page() -> impl IntoView {
	provide_meta_context();
	let is_admin = Resource::new(
		|| (),
		|_| async { crate::fns::commons::is_current_user_admin().await },
	);

	view! {
		<Stylesheet id="devices" href="/static/devices.css" />
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
											"Manage device approvals and trust levels. Untrusted devices appear here after their first connection attempt."
										</p>
									</div>
									<DeviceManagement />
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

#[island]
pub fn DeviceManagement() -> impl IntoView {
	let (search_query, set_search_query) = signal(String::new());
	let (message, set_message) = signal(String::new());
	let (refresh_trigger, set_refresh_trigger) = signal(0);

	let untrusted_devices = Resource::new(
		move || refresh_trigger.get(),
		async |_| crate::fns::devices::list_untrusted().await,
	);

	let search_results = Resource::new(
		move || search_query.get(),
		async |query| {
			if query.trim().is_empty() {
				Ok(vec![])
			} else {
				crate::fns::devices::search(query).await
			}
		},
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
					set_message.set("Device trusted successfully".to_string());
					set_refresh_trigger.update(|n| *n += 1);

					set_timeout(
						move || set_message.set(String::new()),
						std::time::Duration::from_millis(3000),
					);
				}
				Err(e) => {
					set_message.set(format!("Error trusting device: {}", e));
				}
			}
		}
	});

	view! {
		<div class="device-search">
			<h2>"Search Devices by Key"</h2>
			<div class="search-box">
				<input
					type="text"
					placeholder="Paste PEM or hex key fragment..."
					prop:value=move || search_query.get()
					on:input=move |ev| set_search_query.set(event_target_value(&ev))
					class="search-input"
				/>
				<p class="search-help">
					"Search by pasting a key fragment in PEM format or hex (with or without colons)"
				</p>
			</div>

			<Suspense fallback=|| view! { <div class="loading">"Searching..."</div> }>
				{move || {
					let query = search_query.get();
					if query.trim().is_empty() {
						view! {}.into_any()
					} else {
						search_results.get().map(|result| {
							match result {
								Ok(devices) => {
									if devices.is_empty() {
										view! {
											<div class="no-results">"No devices found matching your search"</div>
										}.into_any()
									} else {
										view! {
											<div class="search-results">
												<h3>{format!("Search Results ({} found)", devices.len())}</h3>
												<DeviceTable devices=devices.clone() trust_action=trust_device_action />
											</div>
										}.into_any()
									}
								}
								Err(e) => {
									view! {
										<div class="error">{format!("Search error: {}", e)}</div>
									}.into_any()
								}
							}
						}).unwrap_or_else(|| view! {}.into_any())
					}
				}}
			</Suspense>
		</div>

		{move || {
			let msg = message.get();
			if !msg.is_empty() {
				view! { <div class="message">{msg}</div> }.into_any()
			} else {
				view! {}.into_any()
			}
		}}

		<div class="untrusted-devices">
			<h2>"Untrusted Devices"</h2>
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
									<DeviceTable devices=devices.clone() trust_action=trust_device_action />
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

#[component]
pub fn DeviceTable(
	devices: Vec<crate::fns::devices::DeviceInfo>,
	trust_action: Action<(String, String), Result<(), commons_errors::AppError>>,
) -> impl IntoView {
	view! {
		<div class="device-table">
			<For each=move || devices.clone() key=|device| device.device.id.clone() let:device>
				<DeviceRow device=device.clone() trust_action=trust_action />
			</For>
		</div>
	}
}

#[component]
pub fn DeviceRow(
	device: crate::fns::devices::DeviceInfo,
	trust_action: Action<(String, String), Result<(), commons_errors::AppError>>,
) -> impl IntoView {
	let (key_format, set_key_format) = signal("pem".to_string());
	let (show_history, set_show_history) = signal(false);
	let (selected_role, set_selected_role) = signal("server".to_string());

	let device_id = device.device.id.clone();

	let (history_offset, set_history_offset) = signal(0i64);
	let (all_connections, set_all_connections) =
		signal(HashMap::<String, crate::fns::devices::DeviceConnectionData>::new());
	let (has_more, set_has_more) = signal(false);

	// Get total connection count
	let connection_count = {
		let device_id = device_id.clone();
		Resource::new(
			move || device_id.clone(),
			async |id| crate::fns::devices::connection_count(id).await,
		)
	};

	// Load more connections action
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

	// Effect to handle loading more connections
	Effect::new(move |_| {
		if let Some(result) = load_more_action.value().get() {
			match result {
				Ok(new_connections) => {
					let has_more_data = new_connections.len() == 100;
					set_has_more.set(has_more_data);

					set_all_connections.update(|existing| {
						for conn in new_connections {
							existing.insert(conn.id.clone(), conn);
						}
					});
				}
				Err(_) => {
					set_has_more.set(false);
				}
			}
		}
	});

	// Load initial data when history is shown
	Effect::new(move |_| {
		if show_history.get() && history_offset.get() == 0 && all_connections.get().is_empty() {
			load_more_action.dispatch(0);
		}
	});

	let on_trust_click = {
		let device_id = device_id.clone();
		move |_| {
			let role = selected_role.get();
			trust_action.dispatch((device_id.clone(), role));
		}
	};

	let copy_device_id = {
		let device_id = device.device.id.clone();
		move |_| {
			if let Some(window) = window() {
				let navigator = window.navigator();
				let clipboard = navigator.clipboard();
				let _ = clipboard.write_text(&device_id);
			}
		}
	};

	view! {
		<div class="device-row">
			<div class="device-header">
				<div class="device-info">
					<div class="device-id-section">
						<h3>{device.device.id.clone()}</h3>
						<button class="copy-id-btn" on:click=copy_device_id title="Copy device ID">
							"📋"
						</button>
					</div>
					{device.latest_connection.as_ref().map(|conn| {
						view! {
							<div class="latest-connection-inline">
								<span class="connection-ip">{conn.ip.clone()}</span>
								{conn.user_agent.as_ref().map(|ua| {
									view! {
										<span class="connection-ua">{ua.clone()}</span>
									}
								})}
							</div>
						}
					})}
				</div>

				<div class="device-times">
					<span class="device-first-seen timestamp-hover" title={device.device.created_at.clone()}>
						{format!("First seen: {}", device.device.created_at_relative)}
					</span>
					{device.latest_connection.as_ref().map(|conn| {
						view! {
							<span class="device-last-seen timestamp-hover" title={conn.created_at.clone()}>
								{format!("Last seen: {}", conn.created_at_relative)}
							</span>
						}
					})}
				</div>
			</div>

			<div class="device-keys">
				<div class="keys-header">
					<h4>{format!("Public Keys ({})", device.keys.len())}</h4>
					<div class="format-toggle">
						<label>
							<input
								type="radio"
								name={format!("format-{}", device.device.id)}
								value="hex"
								checked=move || key_format.get() == "hex"
								on:change=move |_| set_key_format.set("hex".to_string())
							/>
							"Hex"
						</label>
						<label>
							<input
								type="radio"
								name={format!("format-{}", device.device.id)}
								value="pem"
								checked=move || key_format.get() == "pem"
								on:change=move |_| set_key_format.set("pem".to_string())
							/>
							"PEM"
						</label>
					</div>
				</div>

				<div class="keys-list">
					<For each=move || device.keys.clone() key=|key| key.id.clone() let:key>
						<div class="key-item">
							{key.name.as_ref().map(|name| {
								view! {
									<div class="key-name">{name.clone()}</div>
								}
							})}
							<div class="key-data">
								{move || {
									if key_format.get() == "pem" {
										view! {
											<pre class="key-pem">{key.pem_data.clone()}</pre>
										}.into_any()
									} else {
										view! {
											<code class="key-hex">{key.hex_data.clone()}</code>
										}.into_any()
									}
								}}
							</div>
						</div>
					</For>
				</div>
			</div>

			<div class="device-actions">
				<div class="trust-device">
					<label for={format!("role-{}", device.device.id)}>"Trust as:"</label>
					<select
						id={format!("role-{}", device.device.id.clone())}
						prop:value=move || selected_role.get()
						on:change=move |ev| set_selected_role.set(event_target_value(&ev))
					>
						<option value="server">"Server"</option>
						<option value="releaser">"Releaser"</option>
					</select>
					<button
						class="trust-btn"
						on:click=on_trust_click
						disabled=move || trust_action.pending().get()
					>
						{move || if trust_action.pending().get() { "Trusting..." } else { "Trust Device" }}
					</button>
				</div>

				<button
					class="history-toggle"
					on:click=move |_| set_show_history.update(|show| *show = !*show)
				>
					{move || {
						let count_text = connection_count.get()
							.and_then(|result| result.ok())
							.map(|count| format!(" ({})", count))
							.unwrap_or_default();

						if show_history.get() {
							format!("Hide History{}", count_text)
						} else {
							format!("Show Connection History{}", count_text)
						}
					}}
				</button>
			</div>

			{move || {
				if show_history.get() {
					view! {
						<details class="connection-history" open=true>
							<summary>"Connection History"</summary>
							<Suspense fallback=|| view! { <div class="loading">"Loading history..."</div> }>
								{move || {
									let connections_map = all_connections.get();
									if connections_map.is_empty() && !load_more_action.pending().get() {
										view! {
											<div class="no-history">"No connection history found"</div>
										}.into_any()
									} else {
										// Convert HashMap to Vec and sort by created_at (descending)
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
														view! {}.into_any()
													}
												}}
											</div>
										}.into_any()
									}
								}}
							</Suspense>
						</details>
					}.into_any()
				} else {
					view! {}.into_any()
				}
			}}
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
		id: &str,
		ip: &str,
		user_agent: Option<&str>,
		time_offset: i32,
	) -> crate::fns::devices::DeviceConnectionData {
		crate::fns::devices::DeviceConnectionData {
			id: id.to_string(),
			created_at: format!("2024-01-01T12:{}:00Z", time_offset.abs()),
			created_at_relative: format!("{}m ago", time_offset.abs()),
			device_id: "test-device".to_string(),
			ip: ip.to_string(),
			user_agent: user_agent.map(|s| s.to_string()),
		}
	}

	#[test]
	fn test_group_consecutive_connections() {
		let connections = vec![
			create_test_connection("1", "192.168.1.1", Some("Chrome"), 1),
			create_test_connection("2", "192.168.1.1", Some("Chrome"), 2),
			create_test_connection("3", "192.168.1.1", Some("Chrome"), 3),
			create_test_connection("4", "192.168.1.2", Some("Chrome"), 4),
			create_test_connection("5", "192.168.1.1", Some("Chrome"), 5),
			create_test_connection("6", "192.168.1.2", Some("Chrome"), 6),
			create_test_connection("7", "192.168.1.2", Some("Chrome"), 7),
			create_test_connection("8", "192.168.1.2", Some("Chrome"), 8),
		];

		let groups = group_consecutive_connections(connections);

		assert_eq!(groups.len(), 4);

		assert_eq!(groups[0].count, 3);
		assert_eq!(groups[0].ip, "192.168.1.1");

		assert_eq!(groups[1].count, 1);
		assert_eq!(groups[1].ip, "192.168.1.2");

		assert_eq!(groups[2].count, 1);
		assert_eq!(groups[2].ip, "192.168.1.1");

		assert_eq!(groups[3].count, 3);
		assert_eq!(groups[3].ip, "192.168.1.2");
	}

	#[test]
	fn test_group_different_user_agents() {
		let connections = vec![
			create_test_connection("1", "192.168.1.1", Some("Chrome"), 1),
			create_test_connection("2", "192.168.1.1", Some("Firefox"), 2),
			create_test_connection("3", "192.168.1.1", Some("Chrome"), 3),
		];

		let groups = group_consecutive_connections(connections);

		assert_eq!(groups.len(), 3);
		assert_eq!(groups[0].count, 1);
		assert_eq!(groups[1].count, 1);
		assert_eq!(groups[2].count, 1);
	}

	#[test]
	fn test_group_empty_connections() {
		let connections = vec![];
		let groups = group_consecutive_connections(connections);
		assert_eq!(groups.len(), 0);
	}

	#[test]
	fn test_hashmap_deduplication() {
		use std::collections::HashMap;

		let mut connections_map = HashMap::new();

		// Add initial connections
		let conn1 = create_test_connection("1", "192.168.1.1", Some("Chrome"), 1);
		let conn2 = create_test_connection("2", "192.168.1.2", Some("Firefox"), 2);
		connections_map.insert(conn1.id.clone(), conn1);
		connections_map.insert(conn2.id.clone(), conn2);
		assert_eq!(connections_map.len(), 2);

		// Add duplicate (same ID) - should replace, not add
		let conn1_duplicate = create_test_connection("1", "192.168.1.1", Some("Chrome"), 1);
		connections_map.insert(conn1_duplicate.id.clone(), conn1_duplicate);
		assert_eq!(connections_map.len(), 2); // Still 2, not 3

		// Convert to sorted vec for grouping (as done in frontend)
		let mut connections_vec: Vec<_> = connections_map.values().cloned().collect();
		connections_vec.sort_by(|a, b| b.created_at.cmp(&a.created_at));

		assert_eq!(connections_vec.len(), 2);
		assert!(connections_vec.iter().any(|c| c.id == "1"));
		assert!(connections_vec.iter().any(|c| c.id == "2"));
	}
}
