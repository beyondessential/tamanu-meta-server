use leptos::prelude::*;
use leptos_meta::{Stylesheet, provide_meta_context};

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

	// Load untrusted devices
	let untrusted_devices = Resource::new(
		move || refresh_trigger.get(),
		async |_| crate::fns::devices::list_untrusted_devices().await,
	);

	// Search results
	let search_results = Resource::new(
		move || search_query.get(),
		async |query| {
			if query.trim().is_empty() {
				Ok(vec![])
			} else {
				crate::fns::devices::search_devices(query).await
			}
		},
	);

	let trust_device_action = Action::new(move |(device_id, role): &(String, String)| {
		let device_id = device_id.clone();
		let role = role.clone();
		async move { crate::fns::devices::trust_device(device_id, role).await }
	});

	// Handle trust device results
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
				"Devices that have connected but haven't been assigned a trust level yet"
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
	let (selected_role, set_selected_role) = signal("admin".to_string());

	let device_id = device.device.id.clone();

	// Load connection history when expanded
	let connection_history = {
		let device_id = device_id.clone();
		Resource::new(
			move || {
				if show_history.get() {
					Some(device_id.clone())
				} else {
					None
				}
			},
			async |maybe_id| {
				if let Some(id) = maybe_id {
					crate::fns::devices::get_device_connection_history(id, Some(10)).await
				} else {
					Ok(vec![])
				}
			},
		)
	};

	let on_trust_click = {
		let device_id = device_id.clone();
		move |_| {
			let role = selected_role.get();
			trust_action.dispatch((device_id.clone(), role));
		}
	};

	view! {
		<div class="device-row">
			<div class="device-header">
				<div class="device-info">
					<h3>"Device " {device.device.id.clone()}</h3>
					<div class="device-meta">
						<span class="device-role">{format!("Role: {}", device.device.role)}</span>
						<span class="device-created timestamp-hover" title={device.device.created_at.clone()}>
							{format!("Created: {}", device.device.created_at_relative)}
						</span>
					</div>
				</div>

				{device.latest_connection.as_ref().map(|conn| {
					view! {
						<div class="latest-connection">
							<h4>"Latest Connection"</h4>
							<div class="connection-details">
								<div class="connection-ip">{format!("IP: {}", conn.ip)}</div>
								<div class="connection-time timestamp-hover" title={conn.created_at.clone()}>{format!("Time: {}", conn.created_at_relative)}</div>
								{conn.user_agent.as_ref().map(|ua| {
									view! {
										<div class="connection-ua">{format!("User Agent: {}", ua)}</div>
									}
								})}
							</div>
						</div>
					}
				})}
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
						<option value="admin">"Admin"</option>
						<option value="releaser">"Releaser"</option>
						<option value="server">"Server"</option>
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
					{move || if show_history.get() { "Hide History" } else { "Show Connection History" }}
				</button>
			</div>

			{move || {
				if show_history.get() {
					view! {
						<details class="connection-history" open=true>
							<summary>"Connection History"</summary>
							<Suspense fallback=|| view! { <div class="loading">"Loading history..."</div> }>
								{move || connection_history.get().map(|result| {
									match result {
										Ok(connections) => {
											if connections.is_empty() {
												view! {
													<div class="no-history">"No connection history found"</div>
												}.into_any()
											} else {
												view! {
													<div class="history-list">
														<For each=move || connections.clone() key=|conn| conn.id.clone() let:conn>
															<div class="history-item">
																<div class="history-time timestamp-hover" title={conn.created_at.clone()}>{conn.created_at_relative.clone()}</div>
																<div class="history-ip">{conn.ip.clone()}</div>
																{conn.user_agent.as_ref().map(|ua| {
																	view! {
																		<div class="history-ua">{ua.clone()}</div>
																	}
																})}
															</div>
														</For>
													</div>
												}.into_any()
											}
										}
										Err(e) => {
											view! {
												<div class="error">{format!("Error loading history: {}", e)}</div>
											}.into_any()
										}
									}
								})}
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
