use commons_types::{Uuid, device::DeviceRole};
use leptos::prelude::*;
use leptos_meta::Title;
use leptos_router::hooks::use_params_map;

use super::history::ConnectionHistory;
use crate::{
	components::{ServerShorty, TimeAgo, ToastCtx},
	fns::devices::DeviceInfo,
};

#[component]
pub fn Detail() -> impl IntoView {
	let params = use_params_map();
	let device_id = move || params.read().get("id").and_then(|s| s.parse::<Uuid>().ok());

	let (refresh_trigger, set_refresh_trigger) = signal(0);
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

	view! {
		<div class="section device-detail">
			<Suspense fallback=|| view! { <div class="loading">"Loading device..."</div> }>
				{move || {
					device_resource.get().map(|result| {
						match result {
							Ok(device_info) => {
								view! {
									<DeviceDetail device_info set_refresh_trigger />
								}.into_any()
							}
							Err(err) => {
								view! {
									<div class="error">
										<h2>"Error Loading Device"</h2>
										<p>{format!("{err}")}</p>
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

#[component]
fn DeviceDetail(device_info: DeviceInfo, set_refresh_trigger: WriteSignal<i32>) -> impl IntoView {
	let device_id = device_info.device.id;
	let device_role = device_info.device.role;
	let ToastCtx(set_message) = use_context().unwrap();

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
		<Title text={ let name = device_info.name(); move || format!("Tamanu Meta Device {name}") } />
		<h1 class="is-size-3">"Device "{device_info.name()}</h1>
		<div class="box">
			<div class="info-grid">
				{device_info.latest_connection.as_ref().map(|conn| {
					view! {
						<div class="info-item">
							<span class="info-label">"Address"</span>
							<span class="info-value monospace">{conn.ip.clone()}</span>
						</div>
					}
				})}
				<div class="info-item">
					<span class="info-label">"First seen"</span>
					<TimeAgo timestamp={device_info.device.created_at} {..} class:info-value />
				</div>
				{device_info.latest_connection.as_ref().map(|conn| {
					view! {
						<div class="info-item">
							<span class="info-label">"Last seen"</span>
							<TimeAgo timestamp={conn.created_at} {..} class:info-value />
						</div>
					}
				})}
				<div class="info-item">
					<span class="info-label">"Last updated"</span>
					<TimeAgo timestamp={device_info.device.updated_at} {..} class:info-value />
				</div>
				{device_info.latest_connection.as_ref().and_then(|conn| conn.user_agent.as_ref()).map(|ua| {
					view! {
						<div class="info-item">
							<span class="info-label">"User-agent"</span>
							<span class="info-value">{ua.clone()}</span>
						</div>
					}
				})}
			</div>
		</div>

		<div class="box">
			<h3 class="is-size-4 mb-3">"Public Keys " <span class="amount is-size-5">{format!("({})", device_info.keys.len())}</span></h3>
			<For each=move || device_info.keys.clone() key=|key| key.id let:key>
				<KeyItem
					key_id=key.id
					name=key.name.clone()
					pem_data=key.pem_data.clone()
					on_update=move || set_refresh_trigger.update(|n| *n += 1)
				/>
			</For>
		</div>

		<div class="box level">
			{if device_role != DeviceRole::Untrusted {
				view! {
					<div class="level-left">
						<div class="level-item">
							<label class="label" for="role">"Change role:"</label>
						</div>
						<div class="level-item">
							<RoleChange
								role=device_role
								action=move |role| drop(trust_action.dispatch((device_id, role)))
								pending=trust_action.pending() />
						</div>
					</div>
					<div class="level-right">
						<div class="level-item">
							<RoleUntrust
								action=move || drop(untrust_action.dispatch(device_id))
								pending=untrust_action.pending() />
						</div>
					</div>
				}.into_any()
			} else {
				view! {
					<div class="level-left">
						<div class="level-item">
							<label class="label" for="role">"Trust this device as:"</label>
						</div>
						<div class="level-item">
							<RoleTrust
								action=move |role| drop(trust_action.dispatch((device_id, role)))
								pending=trust_action.pending() />
							</div>
					</div>
				}.into_any()
			}}
		</div>

		{move || {
			(device_role != DeviceRole::Untrusted).then(|| view! {
				<AssociatedServers device_id />
			})
		}}

		<PastServerAssociations device_id />

		<ConnectionHistory device_id />
	}
}

#[component]
fn RoleChange(
	role: DeviceRole,
	action: impl Fn(DeviceRole) + Copy + Send + 'static,
	pending: Memo<bool>,
) -> impl IntoView {
	let (selected_role, set_selected_role) = signal(role);

	view! {
		<div class="field has-addons">
			<div class="control">
				<div class="select">
					<select
						name="role"
						disabled=move || pending.get()
						prop:value=move || selected_role.get()
						on:change=move |ev| set_selected_role.set(event_target_value(&ev).parse().unwrap_or_default())
					>
						<option value={DeviceRole::Server}>{DeviceRole::Server}</option>
						<option value={DeviceRole::Releaser}>{DeviceRole::Releaser}</option>
						<option value={DeviceRole::Admin}>{DeviceRole::Admin}</option>
					</select>
				</div>
			</div>
			<div class="control">
				<button
					class="button is-primary"
					disabled=move || pending.get()
					on:click=move |_| action(selected_role.get())
				>
					{move || if pending.get() { "Saving..." } else { "Save" }}
				</button>
			</div>
		</div>
	}
}

#[component]
fn RoleTrust(action: impl Fn(DeviceRole) + 'static, pending: Memo<bool>) -> impl IntoView {
	let (selected_role, set_selected_role) = signal(DeviceRole::Server);

	view! {
		<div class="field has-addons">
			<div class="control">
				<div class="select">
					<select
						disabled=move || pending.get()
						prop:value=move || selected_role.get()
						on:change=move |ev| set_selected_role.set(event_target_value(&ev).parse().unwrap_or_default())
					>
						<option value={DeviceRole::Server}>{DeviceRole::Server}</option>
						<option value={DeviceRole::Releaser}>{DeviceRole::Releaser}</option>
						<option value={DeviceRole::Admin}>{DeviceRole::Admin}</option>
					</select>
				</div>
			</div>
			<div class="control">
				<button
					class="button is-primary"
					on:click=move |_| action(selected_role.get())
					disabled=move || pending.get()
				>
					{move || if pending.get() { "Trusting..." } else { "Trust" }}
				</button>
			</div>
		</div>
	}
}

#[component]
fn RoleUntrust(action: impl Fn() + Send + Copy + 'static, pending: Memo<bool>) -> impl IntoView {
	let (show_untrust_confirm, set_show_untrust_confirm) = signal(false);

	view! {
		<div class="field is-grouped">
		{move || if show_untrust_confirm.get() {
			view! {
				<p class="control">
					<button
						class="button is-danger"
						disabled=move || pending.get()
						on:click=move |_| {
							action();
							set_show_untrust_confirm.set(false);
						}
					>
						{move || if pending.get() { "Untrusting..." } else { "Confirm" }}
					</button>
				</p>
				<p class="control">
					<button
						class="button"
						disabled=move || pending.get()
						on:click=move |_| set_show_untrust_confirm.set(false)
					>
						"Cancel"
					</button>
				</p>
			}
			.into_any()
		} else {
			view! {
				<p class="control">
					<button
						class="button is-danger"
						on:click=move |_| set_show_untrust_confirm.set(true)
					>
						"Untrust"
					</button>
				</p>
			}
			.into_any()
		}}
		</div>
	}
}

#[component]
fn KeyItem(
	key_id: Uuid,
	name: Option<String>,
	pem_data: String,
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
		<div class="mt-3">
			{move || {
				let editing_val = editing.get();
				let name_display = name.clone();
				let original_name_for_cancel = original_name.clone();

				if editing_val {
					view! {
						<div class="field is-grouped">
							<p class="control is-expanded">
								<input
									type="text"
									class="input"
									prop:value=move || new_name.get()
									on:input=move |ev| set_new_name.set(event_target_value(&ev))
									placeholder="Key name (optional)"
								/>
							</p>
							<p class="control">
								<button
									class="button is-primary"
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
							</p>
							<p class="control">
								<button
									class="button is-danger"
									on:click=move |_| {
										set_new_name.set(original_name_for_cancel.clone().unwrap_or_default());
										set_editing.set(false);
									}
								>
									"Cancel"
								</button>
							</p>
						</div>
					}.into_any()
				} else {
					view! {
						<div class="level mb-2">
							<div class="level-left">
								<h4 class="level-item is-size-5">{name_display.as_deref().unwrap_or("Unnamed key")}</h4>
							</div>
							<div class="level-right">
								<button
									class="level-item button"
									on:click=move |_| set_editing.set(true)
									title="Edit key name"
								>
									"✏️"
								</button>
							</div>
						</div>
					}.into_any()
				}
			}}
			<pre class="key-data">{pem_data.clone()}</pre>
		</div>
	}
}

#[component]
fn AssociatedServers(device_id: Uuid) -> impl IntoView {
	let servers_resource = Resource::new(
		move || device_id,
		async |id| crate::fns::devices::get_servers_for_device(id).await,
	);

	view! {
		<div class="box">
			<div class="level">
				<div class="level-left">
					<h3 class="is-size-5 level-item">"Associated Servers"</h3>
				</div>
				<div class="level-right">
					<button class="button level-item" on:click=move |_| servers_resource.refetch()>
						"Refresh"
					</button>
				</div>
			</div>
			<Transition fallback=|| view! { <progress class="progress is-small is-primary" max="100">"Loading..."</progress> }>
				{move || {
					servers_resource.get().map(|result| {
						match result {
							Ok(servers) if servers.is_empty() => {
								view! {
									<div class="block has-text-info">"No servers are associated with this device"</div>
								}.into_any()
							}
							Ok(servers) => {
								view! {
									<For each=move || servers.clone() key=|server| server.id let:server>
										<ServerShorty server=server.into() />
									</For>
								}.into_any()
							}
							Err(err) => {
								view! {
									<div class="block has-text-danger">{format!("Error loading servers: {err}")}</div>
								}.into_any()
							}
						}
					})
				}}
			</Transition>
		</div>
	}
}

#[component]
fn PastServerAssociations(device_id: Uuid) -> impl IntoView {
	let past_servers_resource = Resource::new(
		move || device_id,
		async |id| crate::fns::devices::get_past_server_associations(id).await,
	);

	view! {
		<div class="box">
			<div class="level">
				<div class="level-left">
					<h3 class="is-size-5 level-item">"Past Server Associations"</h3>
				</div>
				<div class="level-right">
					<button class="button level-item" on:click=move |_| past_servers_resource.refetch()>
						"Refresh"
					</button>
				</div>
			</div>
			<Transition fallback=|| view! { <progress class="progress is-small is-primary" max="100">"Loading..."</progress> }>
				{move || {
					past_servers_resource.get().map(|result| {
						match result {
							Ok(servers) if servers.is_empty() => {
								view! {
									<div class="block has-text-info">"No past server associations found"</div>
								}.into_any()
							}
							Ok(servers) => {
								view! {
									<For each=move || servers.clone() key=|server| server.id let:server>
										<ServerShorty server=server.into() />
									</For>
								}.into_any()
							}
							Err(err) => {
								view! {
									<div class="block has-text-danger">{format!("Error loading past associations: {err}")}</div>
								}.into_any()
							}
						}
					})
				}}
			</Transition>
		</div>
	}
}
