use leptos::prelude::*;
use leptos::serde_json;
use leptos_meta::Stylesheet;
use leptos_router::hooks::use_params_map;

use crate::app::devices::DeviceListItem;
use crate::components::TimeAgo;
use crate::fns::devices::DeviceInfo;
use crate::fns::servers::{
	ServerDetailData, ServerLastStatusData, ServerListItem, list_all_servers, server_detail,
	update_server,
};

#[component]
pub fn ListPage() -> impl IntoView {
	let servers = Resource::new(|| (), |_| async { list_all_servers().await });

	view! {
		<Stylesheet id="css-servers" href="/static/servers.css" />
		<div id="servers-list-page">
			<div class="page-header">
				<h1>"Servers"</h1>
			</div>
			<Suspense fallback=|| view! { <div class="loading">"Loading servers..."</div> }>
				{move || {
					servers.get().map(|result| {
						match result {
							Ok(server_list) => {
								view! {
									<div class="servers-grid">
										{server_list.into_iter().map(|server| {
											view! {
												<ServerCard server=server />
											}
										}).collect::<Vec<_>>()}
									</div>
								}.into_any()
							}
							Err(e) => {
								view! {
									<div class="error-message">
										{format!("Failed to load servers: {}", e)}
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
fn ServerCard(server: ServerListItem) -> impl IntoView {
	let is_local = server.host.contains("localhost") || server.host.contains(".local");
	let card_class = if is_local {
		"server-card local-server"
	} else {
		"server-card"
	};

	view! {
		<a href={format!("/servers/{}", server.id)} class={card_class}>
			<div class="server-card-header">
				<h3>{server.name.clone().unwrap_or_else(|| "(unnamed)".to_string())}</h3>
				{server.rank.as_ref().map(|rank| {
					let rank = rank.clone();
					view! {
						<span class="server-rank">{rank}</span>
					}
				})}
			</div>
			<div class="server-card-body">
				<div class="server-info">
					<span class="label">"Kind:"</span>
					<span class="value">{server.kind}</span>
				</div>
				<div class="server-info">
					<span class="label">"Host:"</span>
					<span class="value host">{server.host}</span>
				</div>
			</div>
		</a>
	}
}

#[component]
pub fn DetailPage() -> impl IntoView {
	let params = use_params_map();
	let server_id = move || params.read().get("id").unwrap_or_default();

	let detail_resource =
		Resource::new(move || server_id(), async move |id| server_detail(id).await);

	let is_editing = RwSignal::new(false);
	let edit_name = RwSignal::new(String::new());
	let edit_host = RwSignal::new(String::new());
	let edit_rank = RwSignal::new(String::new());
	let edit_device_id = RwSignal::new(String::new());

	let update_action = Action::new(
		move |(name, host, rank, device_id): &(
			Option<String>,
			Option<String>,
			Option<String>,
			Option<String>,
		)| {
			let name = name.clone();
			let host = host.clone();
			let rank = rank.clone();
			let device_id = device_id.clone();
			let id = server_id();
			async move {
				let result = update_server(id, name, host, rank, device_id).await;
				if result.is_ok() {
					is_editing.set(false);
					detail_resource.refetch();
				}
				result
			}
		},
	);

	view! {
		<Stylesheet id="css-deployment" href="/static/deployment.css" />
		<div id="status-detail-page">
			<Suspense fallback=|| view! { <div class="loading">"Loading server details..."</div> }>
				{move || {
					detail_resource.get().map(|result| {
						match result {
							Ok(data) => {
								view! {
									<ServerDetailView
										data=data
										is_editing=is_editing
										edit_name=edit_name
										edit_host=edit_host
										edit_rank=edit_rank
										edit_device_id=edit_device_id
										update_action=update_action
									/>
								}.into_any()
							}
							Err(e) => {
								view! { <ErrorView error=e /> }.into_any()
							}
						}
					})
				}}
			</Suspense>
		</div>
	}
}

#[component]
fn ServerDetailView(
	data: ServerDetailData,
	is_editing: RwSignal<bool>,
	edit_name: RwSignal<String>,
	edit_host: RwSignal<String>,
	edit_rank: RwSignal<String>,
	edit_device_id: RwSignal<String>,
	update_action: Action<
		(
			Option<String>,
			Option<String>,
			Option<String>,
			Option<String>,
		),
		Result<crate::fns::servers::ServerDetailsData, commons_errors::AppError>,
	>,
) -> impl IntoView {
	let server = data.server.clone();
	let device_info = data.device_info.clone();
	let last_status = data.last_status.clone();
	let up = data.up.clone();

	let server_name = server.name.clone();
	let server_host = server.host.clone();
	let server_rank = server.rank.clone();
	let device_id_str = device_info
		.as_ref()
		.map(|d| d.device.id.clone())
		.unwrap_or_default();

	let server_name_for_effect = server_name.clone();
	let server_host_for_effect = server_host.clone();
	let server_rank_for_effect = server_rank.clone();
	let device_id_for_effect = device_id_str.clone();

	Effect::new(move |_| {
		if !is_editing.get() {
			edit_name.set(server_name_for_effect.clone());
			edit_host.set(server_host_for_effect.clone());
			edit_rank.set(server_rank_for_effect.clone());
			edit_device_id.set(device_id_for_effect.clone());
		}
	});

	view! {
		<div class="detail-container">
			<PageHeader
				server_name=server_name.clone()
				server_host=server_host.clone()
				server_rank=server.rank.clone()
				up=up.clone()
				device_id_str=device_id_str.clone()
				is_editing=is_editing
				edit_name=edit_name
				edit_host=edit_host
				edit_rank=edit_rank
				edit_device_id=edit_device_id
			/>

			{move || {
				if is_editing.get() {
					view! {
						<EditForm
							edit_name=edit_name
							edit_host=edit_host
							edit_rank=edit_rank
							edit_device_id=edit_device_id
							update_action=update_action
							is_editing=is_editing
						/>
					}.into_any()
				} else {
					view! {
						<>
							<ServerInfoSection
								host=server.host.clone()
								device_info=device_info.clone()
							/>
							{last_status.as_ref().map(|status| {
								view! {
									<StatusSection status=status.clone() />
								}
							})}
						</>
					}.into_any()
				}
			}}
		</div>
	}
}

#[component]
fn PageHeader(
	server_name: String,
	server_host: String,
	server_rank: String,
	up: String,
	device_id_str: String,
	is_editing: RwSignal<bool>,
	edit_name: RwSignal<String>,
	edit_host: RwSignal<String>,
	edit_rank: RwSignal<String>,
	edit_device_id: RwSignal<String>,
) -> impl IntoView {
	let name_clone = server_name.clone();
	let host_clone = server_host.clone();
	let rank_clone = server_rank.clone();
	let device_id_clone = device_id_str.clone();

	let is_admin = Resource::new(
		|| (),
		|_| async { crate::fns::commons::is_current_user_admin().await },
	);

	view! {
		<div class="page-header">
			<Suspense>
				{move || {
					is_admin.get().and_then(|result| {
						if result.ok().unwrap_or(false) && !is_editing.get() {
							let name = name_clone.clone();
							let host = host_clone.clone();
							let rank = rank_clone.clone();
							let device_id = device_id_clone.clone();
							Some(view! {
								<button
									class="edit-button"
									on:click=move |_| {
										edit_name.set(name.clone());
										edit_host.set(host.clone());
										edit_rank.set(rank.clone());
										edit_device_id.set(device_id.clone());
										is_editing.set(true);
									}
								>
									"Edit"
								</button>
							})
						} else {
							None
						}
					})
				}}
			</Suspense>
			<h1>
				<span class={format!("status-dot {}", up)} title={up.clone()}></span>
				{server_name.clone()}
			</h1>
			<span class="server-rank">{server_rank.clone()}</span>
		</div>
	}
}

#[component]
fn EditForm(
	edit_name: RwSignal<String>,
	edit_host: RwSignal<String>,
	edit_rank: RwSignal<String>,
	edit_device_id: RwSignal<String>,
	update_action: Action<
		(
			Option<String>,
			Option<String>,
			Option<String>,
			Option<String>,
		),
		Result<crate::fns::servers::ServerDetailsData, commons_errors::AppError>,
	>,
	is_editing: RwSignal<bool>,
) -> impl IntoView {
	view! {
		<section class="detail-section edit-form">
			<h2>"Edit Server Details"</h2>
			<form on:submit=move |ev| {
				ev.prevent_default();
				let device_id = {
					let id = edit_device_id.get();
					if id.is_empty() {
						Some(String::new())
					} else {
						Some(id)
					}
				};
				update_action.dispatch((
					Some(edit_name.get()),
					Some(edit_host.get()),
					Some(edit_rank.get()),
					device_id,
				));
			}>
				<div class="form-group">
					<label for="edit-name">"Server Name"</label>
					<input
						type="text"
						id="edit-name"
						prop:value=move || edit_name.get()
						on:input=move |ev| edit_name.set(event_target_value(&ev))
						required
					/>
				</div>

				<div class="form-group">
					<label for="edit-host">"Host URL"</label>
					<input
						type="url"
						id="edit-host"
						prop:value=move || edit_host.get()
						on:input=move |ev| edit_host.set(event_target_value(&ev))
						required
					/>
				</div>

				<div class="form-group">
					<label for="edit-rank">"Server Rank"</label>
					<select
						id="edit-rank"
						prop:value=move || edit_rank.get()
						on:change=move |ev| edit_rank.set(event_target_value(&ev))
						required
					>
						<option value="production" selected=move || edit_rank.get() == "production">"Production"</option>
						<option value="clone" selected=move || edit_rank.get() == "clone">"Clone"</option>
						<option value="demo" selected=move || edit_rank.get() == "demo">"Demo"</option>
						<option value="test" selected=move || edit_rank.get() == "test">"Test"</option>
						<option value="dev" selected=move || edit_rank.get() == "dev">"Dev"</option>
					</select>
				</div>

				<div class="form-group">
					<label for="edit-device-id">"Device ID"</label>
					<input
						type="text"
						id="edit-device-id"
						prop:value=move || edit_device_id.get()
						on:input=move |ev| edit_device_id.set(event_target_value(&ev))
						placeholder="Leave empty to unset"
					/>
					<small class="help-text">"Optional UUID of the device associated with this server"</small>
				</div>

				{move || {
					update_action.value().get().and_then(|result| {
						if let Err(e) = result {
							Some(view! {
								<div class="error-message">
									{format!("Error updating server: {}", e)}
								</div>
							})
						} else {
							None
						}
					})
				}}

				<div class="form-actions">
					<button type="submit" class="save-button" disabled=move || update_action.pending().get()>
						{move || if update_action.pending().get() { "Saving..." } else { "Save" }}
					</button>
					<button
						type="button"
						class="cancel-button"
						on:click=move |_| is_editing.set(false)
						disabled=move || update_action.pending().get()
					>
						"Cancel"
					</button>
				</div>
			</form>
		</section>
	}
}

#[component]
fn ServerInfoSection(host: String, device_info: Option<DeviceInfo>) -> impl IntoView {
	let host_clone = host.clone();
	view! {
		<section class="detail-section">
			<h2>"Central server"</h2>
			<div class="info-grid">
				<div class="info-item">
					<span class="info-label">"Host"</span>
					<span class="info-value">
						<a href={host} target="_blank">{host_clone}</a>
					</span>
				</div>
			</div>
			{device_info.as_ref().map(|device_info| {
				let device_info = device_info.clone();
				view! {
					<div class="device-list-container">
						<h3>"Device"</h3>
						<DeviceListItem device=device_info />
					</div>
				}
			})}
		</section>
	}
}

#[component]
fn StatusSection(status: ServerLastStatusData) -> impl IntoView {
	view! {
		<section class="detail-section">
			<h2>"Latest status"</h2>
			<div class="info-grid">
				<div class="info-item">
					<span class="info-label">"Reported At"</span>
					<TimeAgo timestamp={status.created_at.clone()} {..} class="info-value" />
				</div>
				{status.platform.as_ref().map(|p| {
					let p = p.clone();
					view! {
						<div class="info-item">
							<span class="info-label">"Platform"</span>
							<span class="info-value">{p}</span>
						</div>
					}
				})}
				{status.timezone.as_ref().map(|tz| {
					let tz = tz.clone();
					view! {
						<div class="info-item">
							<span class="info-label">"Timezone"</span>
							<span class="info-value">{tz}</span>
						</div>
					}
				})}
				{status.version.as_ref().map(|v| {
					let v = v.clone();
					view! {
						<div class="info-item">
							<span class="info-label">"Tamanu"</span>
							<span class="info-value monospace">{v}</span>
						</div>
					}
				})}
				{status.postgres.as_ref().map(|pg| {
					let pg = pg.clone();
					view! {
						<div class="info-item">
							<span class="info-label">"PostgreSQL"</span>
							<span class="info-value monospace">{pg}</span>
						</div>
					}
				})}
				{status.nodejs.as_ref().map(|node| {
					let node = node.clone();
					view! {
						<div class="info-item">
							<span class="info-label">"Node.js"</span>
							<span class="info-value monospace">{node}</span>
						</div>
					}
				})}
			</div>

			{
				let extra = status.extra.clone();
				if !extra.as_object().map(|o| o.is_empty()).unwrap_or(true) {
					view! {
						<details class="extra-data">
							<summary>"Extra Data"</summary>
							<pre class="json-display">{serde_json::to_string_pretty(&extra).unwrap_or_default()}</pre>
						</details>
					}.into_any()
				} else {
					().into_any()
				}
			}
		</section>
	}
}

#[component]
fn ErrorView(error: commons_errors::AppError) -> impl IntoView {
	let error_msg = format!("Failed to load server details: {}", error);
	view! {
		<div class="error-container">
			<div class="page-header">
				<h1>"Error"</h1>
			</div>
			<div class="error-message">
				{error_msg}
			</div>
		</div>
	}
}
