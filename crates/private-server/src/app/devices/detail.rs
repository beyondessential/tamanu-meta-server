use commons_types::{Uuid, device::DeviceRole};
use leptos::prelude::*;
use leptos_meta::Title;
use leptos_router::hooks::use_params_map;
use web_sys::window;

use crate::components::ToastCtx;

#[component]
fn KeyItem(
	key_id: Uuid,
	_device_id: Uuid,
	name: Option<String>,
	pem_data: String,
	hex_data: String,
	#[prop(into)] key_format: Signal<String>,
	on_update: impl Fn() + 'static + Copy,
) -> impl IntoView {
	let ToastCtx(set_message) = use_context().unwrap();
	let (editing, set_editing) = signal(false);
	let (new_name, set_new_name) = signal(name.clone().unwrap_or_default());

	let update_key_name_action = Action::new(move |(key_id, name): &(Uuid, Option<String>)| {
		let key_id = *key_id;
		let name = name.clone();
		async move { crate::fns::devices::update_key_name(key_id, name).await }
	});

	Effect::new(move |_| {
		if let Some(result) = update_key_name_action.value().get() {
			match result {
				Ok(_) => {
					set_message.set(Some("Key name updated successfully".to_string()));
					set_editing.set(false);
					on_update();
					set_timeout(
						move || set_message.set(None),
						std::time::Duration::from_millis(3000),
					);
				}
				Err(e) => {
					set_message.set(Some(format!("Error updating key name: {}", e)));
				}
			}
		}
	});

	let original_name = name.clone();

	view! {
		<div class="key-item">
			{move || {
				let editing_val = editing.get();
				let name_display = name.clone();
				let original_name_for_cancel = original_name.clone();

				if editing_val {
					view! {
						<div class="key-name-edit">
							<input
								type="text"
								class="key-name-input"
								prop:value=move || new_name.get()
								on:input=move |ev| set_new_name.set(event_target_value(&ev))
								placeholder="Key name (optional)"
							/>
							<button
								class="key-name-save-btn"
								on:click=move |_| {
									let name_value = new_name.get().trim().to_string();
									let name_to_save = if name_value.is_empty() {
										None
									} else {
										Some(name_value)
									};
									update_key_name_action.dispatch((key_id, name_to_save));
								}
								disabled=move || update_key_name_action.pending().get()
							>
								{move || if update_key_name_action.pending().get() { "Saving..." } else { "Save" }}
							</button>
							<button
								class="key-name-cancel-btn"
								on:click=move |_| {
									set_new_name.set(original_name_for_cancel.clone().unwrap_or_default());
									set_editing.set(false);
								}
							>
								"Cancel"
							</button>
						</div>
					}.into_any()
				} else {
					view! {
						<div class="key-name-display">
							{name_display.as_ref().map(|n| {
								view! {
									<div class="key-name">{n.clone()}</div>
								}.into_any()
							}).unwrap_or_else(|| {
								view! {
									<div class="key-name key-name-empty">"Unnamed key"</div>
								}.into_any()
							})}
							<button
								class="key-name-edit-btn"
								on:click=move |_| set_editing.set(true)
								title="Edit key name"
							>
								"✏️"
							</button>
						</div>
					}.into_any()
				}
			}}
			<div class="key-data">
				{move || {
					if key_format.get() == "pem" {
						view! {
							<pre class="key-pem">{pem_data.clone()}</pre>
						}.into_any()
					} else {
						view! {
							<code class="key-hex">{hex_data.clone()}</code>
						}.into_any()
					}
				}}
			</div>
		</div>
	}
}

#[component]
pub fn Detail() -> impl IntoView {
	let ToastCtx(set_message) = use_context().unwrap();
	let params = use_params_map();
	let device_id = move || {
		params
			.read()
			.get("id")
			.map(|s| s.parse::<Uuid>().ok())
			.flatten()
	};

	let (refresh_trigger, set_refresh_trigger) = signal(0);
	let (key_format, set_key_format) = signal("pem".to_string());
	let (show_history, set_show_history) = signal(false);
	let (show_untrust_confirm, set_show_untrust_confirm) = signal(false);

	let device_resource = Resource::new(
		move || (device_id(), refresh_trigger.get()),
		async |(id, _)| {
			if let Some(id) = id {
				crate::fns::devices::get_device_by_id(id).await
			} else {
				Err(commons_errors::AppError::custom("No device ID provided"))
			}
		},
	);

	let update_role_action = Action::new(move |(device_id, role): &(Uuid, DeviceRole)| {
		let device_id = *device_id;
		let role = *role;
		async move { crate::fns::devices::update_role(device_id, role).await }
	});

	Effect::new(move |_| {
		if let Some(result) = update_role_action.value().get() {
			match result {
				Ok(_) => {
					set_message.set(Some("Device role updated successfully".to_string()));
					set_refresh_trigger.update(|n| *n += 1);
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

	let untrust_action = Action::new(move |device_id: &Uuid| {
		let device_id = *device_id;
		async move { crate::fns::devices::untrust(device_id).await }
	});

	Effect::new(move |_| {
		if let Some(result) = untrust_action.value().get() {
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

	let trust_action = Action::new(move |(device_id, role): &(Uuid, DeviceRole)| {
		let device_id = *device_id;
		let role = *role;
		async move { crate::fns::devices::trust(device_id, role).await }
	});

	Effect::new(move |_| {
		if let Some(result) = trust_action.value().get() {
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
		<Title text=move || format!("Tamanu Meta Device {}", device_id().map(|id| id.to_string()).unwrap_or_default()) />
		<div class="device-detail">
			<Suspense fallback=|| view! { <div class="loading">"Loading device..."</div> }>
				{move || {
					device_resource.get().map(|result| {
						match result {
							Ok(device_info) => {
								let device_id = device_info.device.id.clone();
								let device_id_for_untrust = device_id.clone();
								let device_id_for_trust = device_id.clone();
								let device_id_for_update = device_id.clone();
								let device_role = device_info.device.role;
								let default_role = if device_role != DeviceRole::Untrusted {
									device_role
								} else {
									DeviceRole::Server
								};
								let (selected_role, set_selected_role) = signal(default_role);

								let copy_device_id = {
									let device_id = device_id.clone();
									move |_| {
										if let Some(window) = window() {
											let navigator = window.navigator();
											let clipboard = navigator.clipboard();
											let _ = clipboard.write_text(&device_id.to_string());
										}
									}
								};

								view! {
									<div class="device-detail-content">
										<div class="device-header">
											<div class="device-info">
												<div class="device-id-section">
													<h2>
														{device_info.device.id.to_string()}
														<span class="role-badge-header">{device_info.device.role.to_string()}</span>
													</h2>
													<button class="copy-id-btn" on:click=copy_device_id title="Copy device ID">
														"📋"
													</button>
												</div>
												{device_info.latest_connection.as_ref().map(|conn| {
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
												<span class="device-first-seen timestamp-hover" title={device_info.device.created_at.clone()}>
													{format!("First seen: {}", device_info.device.created_at_relative)}
												</span>
												{device_info.latest_connection.as_ref().map(|conn| {
													view! {
														<span class="device-last-seen timestamp-hover" title={conn.created_at.clone()}>
															{format!("Last seen: {}", conn.created_at_relative)}
														</span>
													}
												})}
												<span class="device-last-updated timestamp-hover" title={device_info.device.updated_at.clone()}>
													{format!("Last updated: {}", device_info.device.updated_at_relative)}
												</span>
											</div>
										</div>

										<div class="device-keys">
											<div class="keys-header">
												<h3>{format!("Public Keys ({})", device_info.keys.len())}</h3>
												<div class="format-toggle">
													<label>
														<input
															type="radio"
															name="format"
															value="hex"
															checked=move || key_format.get() == "hex"
															on:change=move |_| set_key_format.set("hex".to_string())
														/>
														"Hex"
													</label>
													<label>
														<input
															type="radio"
															name="format"
															value="pem"
															checked=move || key_format.get() == "pem"
															on:change=move |_| set_key_format.set("pem".to_string())
														/>
														"PEM"
													</label>
												</div>
											</div>

											<div class="keys-list">
												<For each=move || device_info.keys.clone() key=|key| key.id.clone() let:key>
													<KeyItem
														key_id=key.id
														_device_id=device_id.clone()
														name=key.name.clone()
														pem_data=key.pem_data.clone()
														hex_data=key.hex_data.clone()
														key_format
														on_update=move || set_refresh_trigger.update(|n| *n += 1)
													/>
												</For>
											</div>
										</div>

										<div class="device-actions">
											{if device_role != DeviceRole::Untrusted {
												view! {
													<div class="trusted-device-actions">
														<div class="actions-row">
															<label>"Change Role:"</label>
															<select
																prop:value=move || selected_role.get().to_string()
																on:change=move |ev| set_selected_role.set(event_target_value(&ev).parse().unwrap_or_default())
															>
																<option value={DeviceRole::Server.to_string()}>"Server"</option>
																<option value={DeviceRole::Releaser.to_string()}>"Releaser"</option>
																<option value={DeviceRole::Admin.to_string()}>"Admin"</option>
															</select>
															<button
																class="update-role-btn"
																on:click={
																	let device_id = device_id_for_update.clone();
																	move |_| {
																		let role = selected_role.get();
																		update_role_action.dispatch((device_id, role));
																	}
																}
																disabled={
																	move || {
																		update_role_action.pending().get() || selected_role.get() == device_role
																	}
																}
															>
																{move || if update_role_action.pending().get() { "Updating..." } else { "Update Role" }}
															</button>
														</div>

														<div class="actions-row">
															{move || {
																if show_untrust_confirm.get() {
																	let device_id = device_id_for_untrust.clone();
																	view! {
																		<div class="untrust-confirm-inline">
																			<span class="confirm-text">"Are you sure?"</span>
																			<button
																				class="untrust-confirm-btn"
																				on:click=move |_| {
																					untrust_action.dispatch(device_id);
																					set_show_untrust_confirm.set(false);
																				}
																				disabled=move || untrust_action.pending().get()
																			>
																				{move || if untrust_action.pending().get() { "Untrusting..." } else { "Yes, Untrust" }}
																			</button>
																			<button
																				class="untrust-cancel-btn"
																				on:click=move |_| set_show_untrust_confirm.set(false)
																			>
																				"Cancel"
																			</button>
																		</div>
																	}.into_any()
																} else {
																	view! {
																		<button
																			class="untrust-btn"
																			on:click=move |_| set_show_untrust_confirm.set(true)
																		>
																			"Untrust Device"
																		</button>
																	}.into_any()
																}
															}}
														</div>
													</div>
												}.into_any()
											} else {
												view! {
													<div class="trust-device">
														<label>"Trust this device as:"</label>
														<select
															prop:value=move || selected_role.get().to_string()
															on:change=move |ev| set_selected_role.set(event_target_value(&ev).parse().unwrap_or_default())
														>
															<option value={DeviceRole::Server.to_string()}>"Server"</option>
															<option value={DeviceRole::Releaser.to_string()}>"Releaser"</option>
															<option value={DeviceRole::Admin.to_string()}>"Admin"</option>
														</select>
														<button
															class="trust-btn"
															on:click={
																let device_id = device_id_for_trust.clone();
																move |_| {
																	let role = selected_role.get();
																	trust_action.dispatch((device_id, role));
																}
															}
															disabled=move || trust_action.pending().get()
														>
															{move || if trust_action.pending().get() { "Trusting..." } else { "Trust Device" }}
														</button>
													</div>
												}.into_any()
											}}

											<button
												class="history-toggle"
												on:click=move |_| set_show_history.update(|show| *show = !*show)
											>
												{move || {
													if show_history.get() {
														"Hide Connection History"
													} else {
														"Show Connection History"
													}
												}}
											</button>
										</div>

										{move || {
											if show_history.get() {
												view! {
													<super::DeviceConnectionHistory device_id />
												}.into_any()
											} else {
												().into_any()
											}
										}}
									</div>

									<super::AssociatedServers device_id device_role />
								}.into_any()
							}
							Err(e) => {
								view! {
									<div class="error">
										<h2>"Error Loading Device"</h2>
										<p>{format!("{}", e)}</p>
										<a href="/devices" class="back-link">"← Back to Devices"</a>
									</div>
								}.into_any()
							}
						}
					})
				}}
			</Suspense>
		</div>
	}
}
