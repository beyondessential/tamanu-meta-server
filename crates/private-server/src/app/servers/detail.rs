use std::sync::Arc;

use commons_types::{
	Uuid,
	server::{kind::ServerKind, rank::ServerRank},
};
use leptos::prelude::*;
use leptos::serde_json;
use leptos_meta::Stylesheet;
use leptos_router::components::A;
use leptos_router::components::Redirect;
use leptos_router::hooks::use_params_map;

use crate::app::devices::DeviceListItem;
use crate::components::{StatusLegend, TimeAgo, VersionIndicator, VersionLegend};
use crate::fns::servers::{
	ChildServerData, ServerDetailData, ServerLastStatusData, assign_parent_server,
	search_central_servers, server_detail, update_server,
};

fn is_admin_resource() -> Resource<bool> {
	Resource::new(
		|| (),
		|_| async {
			crate::fns::commons::is_current_user_admin()
				.await
				.unwrap_or(false)
		},
	)
}

#[component]
pub fn Detail() -> impl IntoView {
	let params = use_params_map();
	let server_id = move || {
		params
			.read()
			.get("id")
			.map(|id| id.parse::<Uuid>().ok())
			.flatten()
			.unwrap_or_default()
	};

	let detail_resource =
		Resource::new(move || server_id(), async move |id| server_detail(id).await);

	let is_editing = RwSignal::new(false);
	let edit_name = RwSignal::new(String::new());
	let edit_host = RwSignal::new(String::new());
	let edit_rank = RwSignal::new(None::<ServerRank>);
	let edit_device_id = RwSignal::new(None::<Uuid>);
	let edit_parent_id = RwSignal::new(None::<Uuid>);
	let edit_listed = RwSignal::new(false);

	let is_admin = is_admin_resource();

	let update_action = Action::new(
		move |(name, host, rank, device_id, parent_id, listed): &(
			Option<String>,
			Option<String>,
			Option<ServerRank>,
			Option<Uuid>,
			Option<Uuid>,
			Option<bool>,
		)| {
			let name = name.clone();
			let host = host.clone();
			let id = server_id();
			let rank = *rank;
			let device_id = *device_id;
			let parent_id = *parent_id;
			let listed = *listed;
			async move {
				let result =
					update_server(id, name, host, rank, device_id, parent_id, listed).await;
				if result.is_ok() {
					is_editing.set(false);
					detail_resource.refetch();
				}
				result
			}
		},
	);

	view! {
		<Stylesheet id="css-servers" href="/static/servers.css" />
		<div id="status-detail-page">
			<Suspense fallback=|| view! { <div class="loading">"Loading server details..."</div> }>
				{move || {
					detail_resource.get().map(|result| {
						match result {
							Ok(data) => {
								// If server is not central and has a parent, redirect to parent (unless editing)
								if data.server.kind != ServerKind::Central && !is_editing.get() {
									if let Some(parent_id) = &data.server.parent_server_id {
										return view! {
											<Redirect path={format!("/servers/{}", parent_id)} />
										}.into_any();
									}
								}

								view! {
									<ServerDetailView
										data
										is_editing
										is_admin
										edit_name
										edit_host
										edit_rank
										edit_device_id
										edit_parent_id
										edit_listed
										update_action
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
	is_admin: Resource<bool>,
	edit_name: RwSignal<String>,
	edit_host: RwSignal<String>,
	edit_rank: RwSignal<Option<ServerRank>>,
	edit_device_id: RwSignal<Option<Uuid>>,
	edit_parent_id: RwSignal<Option<Uuid>>,
	edit_listed: RwSignal<bool>,
	update_action: Action<
		(
			Option<String>,
			Option<String>,
			Option<ServerRank>,
			Option<Uuid>,
			Option<Uuid>,
			Option<bool>,
		),
		Result<crate::fns::servers::ServerDetailsData, commons_errors::AppError>,
	>,
) -> impl IntoView {
	let data = Arc::new(data);

	Effect::new({
		let data = data.clone();
		move |_| {
			if !is_editing.get() {
				edit_name.set(data.server.name.clone());
				edit_host.set(data.server.host.clone());
				edit_rank.set(data.server.rank);
				edit_device_id.set(data.device_info.as_ref().map(|d| d.device.id));
				edit_parent_id.set(data.server.parent_server_id);
				edit_listed.set(data.server.listed);
			}
		}
	});

	view! {
		<div class="detail-container">
			<PageHeader
				data=data.clone()
				is_admin
				is_editing
				edit_name
				edit_host
				edit_rank
				edit_device_id
				_edit_parent_id=edit_parent_id
			/>

			{move || {
				if is_editing.get() {
					view! {
						<EditForm
							data=data.clone()
							edit_name
							edit_host
							edit_rank
							edit_device_id
							edit_parent_id
							edit_listed
							update_action
							is_editing
						/>
					}.into_any()
				} else {
					view! {
						<>
							<ServerInfoSection data=data.clone() />
							{data.last_status.as_ref().map(|status| {
								view! {
									<StatusSection status=status.clone() />
								}
							})}
							{if data.server.kind == ServerKind::Central && !data.child_servers.is_empty() {
								view! {
									<ChildServersSection data=data.clone() is_admin />
								}.into_any()
							} else if data.server.kind != ServerKind::Central && data.server.parent_server_id.is_none() {
								view! {
									<AssignParentSection server_id=data.server.id.clone() />
								}.into_any()
							} else {
								().into_any()
							}}
						</>
					}.into_any()
				}
			}}

			<div class="legends-container">
				<VersionLegend />
				<StatusLegend />
			</div>
		</div>
	}
}

#[component]
fn PageHeader(
	data: Arc<ServerDetailData>,
	is_admin: Resource<bool>,
	is_editing: RwSignal<bool>,
	edit_name: RwSignal<String>,
	edit_host: RwSignal<String>,
	edit_rank: RwSignal<Option<ServerRank>>,
	edit_device_id: RwSignal<Option<Uuid>>,
	_edit_parent_id: RwSignal<Option<Uuid>>,
) -> impl IntoView {
	let suspense_data = data.clone();
	view! {
		<div class="page-header">
			<Suspense>
				{ let data = suspense_data.clone(); move || {
					if is_admin.get().unwrap_or(false) && !is_editing.get() {
						let data = data.clone();
						Some(view! {
							<button
								class="edit-button"
								on:click={ let data = data.clone(); move |_| {
									edit_name.set(data.server.name.clone());
									edit_host.set(data.server.host.clone());
									edit_rank.set(data.server.rank);
									edit_device_id.set(data.device_info.as_ref().map(|d| d.device.id));
									// edit_parent_id already set by Effect
									is_editing.set(true);
								}}
							>
								"Edit"
							</button>
						})
					} else {
						None
					}
				}}
			</Suspense>
			<h1>
				<span class={format!("status-dot {}", data.up)} title={format!("{}: {}", data.server.name, data.up)}></span>
				{if data.server.kind == ServerKind::Central {
					data.child_servers.iter().map(|child| {
						let child = child.clone();
						view! {
							<span
								class={format!("status-dot facility-dot {}", child.up)}
								title={format!("{}: {}", child.name, child.up)}
							></span>
						}
					}).collect_view().into_any()
				} else {
					().into_any()
				}}
				{data.server.name.clone()}
			</h1>
			<span class="server-rank">{data.server.rank.map(|r| r.to_string()).unwrap_or_default()}</span>
		</div>
	}
}

#[component]
fn EditForm(
	data: Arc<ServerDetailData>,
	edit_name: RwSignal<String>,
	edit_host: RwSignal<String>,
	edit_rank: RwSignal<Option<ServerRank>>,
	edit_device_id: RwSignal<Option<Uuid>>,
	edit_parent_id: RwSignal<Option<Uuid>>,
	edit_listed: RwSignal<bool>,
	update_action: Action<
		(
			Option<String>,
			Option<String>,
			Option<ServerRank>,
			Option<Uuid>,
			Option<Uuid>,
			Option<bool>,
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
				let device_id = edit_device_id.get();
				let parent_id = edit_parent_id.get();
				let listed = edit_listed.get();
				update_action.dispatch((
					Some(edit_name.get()),
					Some(edit_host.get()),
					edit_rank.get(),
					device_id,
					parent_id,
					Some(listed),
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
						prop:value=move || edit_rank.get().map(|r| r.to_string()).unwrap_or_default()
						on:change=move |ev| edit_rank.set(event_target_value(&ev).parse().ok())
						required
					>
						<option value={ServerRank::Production.to_string()} selected=move || edit_rank.get() == Some(ServerRank::Production)>"Production"</option>
						<option value={ServerRank::Clone.to_string()} selected=move || edit_rank.get() == Some(ServerRank::Clone)>"Clone"</option>
						<option value={ServerRank::Demo.to_string()} selected=move || edit_rank.get() == Some(ServerRank::Demo)>"Demo"</option>
						<option value={ServerRank::Test.to_string()} selected=move || edit_rank.get() == Some(ServerRank::Test)>"Test"</option>
						<option value={ServerRank::Dev.to_string()} selected=move || edit_rank.get() == Some(ServerRank::Dev)>"Dev"</option>
					</select>
				</div>

				<div class="form-group">
					<label for="edit-device-id">"Device ID"</label>
					<input
						type="text"
						id="edit-device-id"
						prop:value=move || edit_device_id.get().map(|id| id.to_string()).unwrap_or_default()
						on:input=move |ev| edit_device_id.set(event_target_value(&ev).parse().ok())
						placeholder="Leave empty to unset"
					/>
					<small class="help-text">"Optional UUID of the device associated with this server"</small>
				</div>

				{if data.server.kind != ServerKind::Central {
					view! {
						<div class="form-group">
							<label for="edit-parent-id">"Parent Server ID"</label>
							<input
								type="text"
								id="edit-parent-id"
								prop:value=move || edit_parent_id.get().map(|id| id.to_string()).unwrap_or_default()
								on:input=move |ev| edit_parent_id.set(event_target_value(&ev).parse().ok())
								placeholder="Leave empty to unset parent"
							/>
							<small class="help-text">"Optional UUID of the parent central server"</small>
						</div>
					}.into_any()
				} else {
					view! {
						<div class="form-group">
							<label for="edit-listed">
								<input
									type="checkbox"
									id="edit-listed"
									prop:checked=move || edit_listed.get()
									on:change=move |ev| edit_listed.set(event_target_checked(&ev))
								/>
								" Listed in Tamanu mobile"
							</label>
							<small class="help-text">"When checked, this server will appear in the public Tamanu mobile server list"</small>
						</div>
					}.into_any()
				}}

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
fn ServerInfoSection(data: Arc<ServerDetailData>) -> impl IntoView {
	view! {
		<section class="detail-section">
			<h2>
				<span class={format!("section-status-dot status-dot {}", data.up)}></span>
				"Central server"
				<a class="header-url" href={data.server.host.clone()} target="_blank">{data.server.host.clone()}</a>
			</h2>
			{data.device_info.as_ref().map(|device_info| {
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
fn StatusSection(status: Arc<ServerLastStatusData>) -> impl IntoView {
	let min_chrome_version = status.min_chrome_version;
	view! {
		<section class:detail-section>
			<h2>"Latest status"</h2>
			<div class:info-grid>
				<div class:info-item>
				<span class="info-label">"Reported At"</span>
				<TimeAgo timestamp={status.created_at.clone()} {..} class:info-value />
				</div>
				{status.platform.as_ref().map(|p| {
					let p = p.clone();
					view! {
						<div class:info-item>
						<span class="info-label">"Platform"</span>
						<span class:info-value>{p}</span>
						</div>
					}
				})}
				{status.timezone.as_ref().map(|tz| {
					let tz = tz.clone();
					view! {
						<div class:info-item>
							<span class="info-label">"Timezone"</span>
							<span class:info-value>{tz}</span>
						</div>
					}
				})}
				{status.version.as_ref().map(|v| {
					let v = v.clone();
					let distance = status.version_distance;

					view! {
						<div class:info-item class:version>
							<span class="info-label">"Tamanu"</span>
							<span class:info-value class:monospace>
								<VersionIndicator version={v} distance={distance} />
							</span>
						</div>
					}
				})}
				{status.postgres.as_ref().map(|pg| {
					let pg = pg.clone();
					view! {
						<div class:info-item class:version>
							<span class="info-label">"PostgreSQL"</span>
							<span class:info-value class:monospace>{pg}</span>
						</div>
					}
				})}
				{status.nodejs.as_ref().map(|node| {
					let node = node.clone();
					view! {
						<div class:info-item class:version>
							<span class="info-label">"Node.js"</span>
							<span class:info-value class:monospace>{node}</span>
						</div>
					}
				})}
				{min_chrome_version.map(|chrome_ver| {
					view! {
						<div class="info-item version">
							<span class="info-label">"Chrome"</span>
							<span class="info-value monospace">{format!("{chrome_ver} or later")}</span>
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
fn ChildServersSection(data: Arc<ServerDetailData>, is_admin: Resource<bool>) -> impl IntoView {
	view! {
		<section class="detail-section">
			<h2>"Facility servers (" {data.child_servers.len()} ")"</h2>
			<div class="child-servers-list">
				{data.child_servers.iter().map(|child| {
					view! {
						<ChildServerCard child=child.clone() is_admin />
					}
				}).collect::<Vec<_>>()}
			</div>
		</section>
	}
}

#[component]
fn ChildServerCard(child: Arc<ChildServerData>, is_admin: Resource<bool>) -> impl IntoView {
	let child_id = child.id;
	view! {
		<div class="child-server-card">
			<div class="child-server-header">
				<span class={format!("status-dot {}", child.up)} title={child.up.to_string()}></span>
				<a href={format!("/servers/{}", child.id)} class="child-server-name">
					{child.name.clone()}
				</a>
				<Suspense>
					{move || {
						if is_admin.get().unwrap_or(false) {
							Some(view! {
								<A class:edit-button href={format!("/servers/{child_id}/edit")}>
									"Edit"
								</A>
							})
						} else {
							None
						}
					}}
				</Suspense>
			</div>
			<div class="detail-host">
				"Link: "
				<a href={child.host.clone()} target="_blank">{child.host.clone()}</a>
			</div>
			{child.device_info.as_ref().map(|device_info| {
				let device_info = device_info.clone();
				view! {
					<div class="detail-device">
						"Device: "
						<A href=format!("/devices/{}", device_info.device.id)>{
							device_info.latest_connection.as_ref().map(|c| c.ip.to_string()).unwrap_or_else(|| device_info.device.role.to_string())
						}</A>
					</div>
				}
			})}
			{child.last_status.as_ref().map(|status| {
				let status = status.clone();
				view! {
					<div class:child-server-details>
						<div class:child-server-info>
							<div class:info-item>
								<span class="info-label">"Rank"</span>
								<span class:info-value>{child.rank.map(|r| r.to_string()).unwrap_or_default()}</span>
							</div>
							<div class:info-item>
								<span class="info-label">"Reported At"</span>
								<TimeAgo timestamp={status.created_at.clone()} {..} class:info-value />
							</div>
							{status.platform.as_ref().map(|p| {
								let p = p.clone();
								view! {
									<div class:info-item>
										<span class="info-label">"Platform"</span>
										<span class:info-value>{p}</span>
									</div>
								}
							})}
						</div>
						<div class:child-status-info>
							{status.version.as_ref().map(|v| {
								let v = v.clone();
								view! {
									<div class:info-item class:version>
										<span class="info-label">"Tamanu"</span>
										<span class:info-value>{v.to_string()}</span>
									</div>
								}
							})}
							{status.postgres.as_ref().map(|pg| {
								let pg = pg.clone();
								view! {
									<div class:info-item class:version>
										<span class="info-label">"PostgreSQL"</span>
										<span class:info-value>{pg}</span>
									</div>
								}
							})}
							{status.nodejs.as_ref().map(|node| {
								let node = node.clone();
								view! {
									<div class:info-item class:version>
										<span class="info-label">"Node.js"</span>
										<span class:info-value>{node}</span>
									</div>
								}
							})}
						</div>
					</div>
				}
			})}
		</div>
	}
}

#[component]
fn AssignParentSection(server_id: Uuid) -> impl IntoView {
	let search_query = RwSignal::new(String::new());
	let search_results = RwSignal::new(Vec::new());

	let current_rank = RwSignal::new(None::<ServerRank>);

	let is_admin = is_admin_resource();

	let detail_resource = Resource::new(move || server_id, async move |id| server_detail(id).await);

	Effect::new(move |_| {
		if let Some(Ok(data)) = detail_resource.get() {
			current_rank.set(data.server.rank);
		}
	});

	let assign_action = Action::new(move |parent_id: &Uuid| {
		let parent_id = *parent_id;
		async move {
			let result = assign_parent_server(server_id, parent_id).await;
			if result.is_ok() {
				leptos_router::hooks::use_navigate()(
					&format!("/servers/{}", parent_id),
					Default::default(),
				);
			}
			result
		}
	});

	let search_action = Action::new(move |query: &String| {
		let query = query.clone();
		async move {
			if query.is_empty() {
				search_results.set(Vec::new());
				Ok(())
			} else {
				match search_central_servers(query).await {
					Ok(results) => {
						search_results.set(results);
						Ok(())
					}
					Err(e) => Err(e),
				}
			}
		}
	});

	view! {
		<section class="detail-section">
			<Suspense>
				{move || {
					if is_admin.get().unwrap_or(false) {
							Some(view! {
								<>
									<h2>"Assign Parent Server"</h2>
									<p class="help-text">"This server does not have a parent server. Search and select a central server to assign as parent."</p>
									<div class="parent-search">
										<input
											type="text"
											placeholder="Search for central server..."
											prop:value=move || search_query.get()
											on:input=move |ev| {
												let query = event_target_value(&ev);
												search_query.set(query.clone());
												search_action.dispatch(query);
											}
										/>
										{move || {
											if search_action.pending().get() {
												view! { <div class="search-status">"Searching..."</div> }.into_any()
											} else if !search_results.get().is_empty() {
												let rank = current_rank.get();
												let mut results = search_results.get();

												// Sort: matching rank first, then others
												results.sort_by(|a, b| {
													let a_matches = a.rank == rank;
													let b_matches = b.rank == rank;
													match (a_matches, b_matches) {
														(true, false) => std::cmp::Ordering::Less,
														(false, true) => std::cmp::Ordering::Greater,
														_ => std::cmp::Ordering::Equal,
													}
												});

												view! {
													<div class="search-results">
														{results.into_iter().map(|server| {
															let server_id = server.id.clone();
															let rank_matches = server.rank == rank;
															let opacity_class = if rank_matches { "" } else { "faded" };
															view! {
																<div class={format!("search-result-item {}", opacity_class)}>
																	<div class="search-result-info">
																		<strong>{server.name.unwrap_or_else(|| "(unnamed)".to_string())}</strong>
																		<span class="search-result-host">{server.host}</span>
																		{server.rank.as_ref().map(|rank| {
																			view! {
																				<span class="search-result-rank">{rank.to_string()}</span>
																			}
																		})}
																	</div>
																	<button
																		class="assign-button"
																		on:click=move |_| {
																			assign_action.dispatch(server_id.clone());
																		}
																		disabled=move || assign_action.pending().get()
																	>
																		"Assign"
																	</button>
																</div>
															}
														}).collect::<Vec<_>>()}
													</div>
												}.into_any()
											} else if !search_query.get().is_empty() {
												view! { <div class="search-status">"No central servers found"</div> }.into_any()
											} else {
												().into_any()
											}
										}}
										{move || {
											assign_action.value().get().and_then(|result| {
												if let Err(e) = result {
													Some(view! {
														<div class="error-message">
															{format!("Error assigning parent: {}", e)}
														</div>
													})
												} else {
													None
												}
											})
										}}
									</div>
								</>
							}.into_any())
						} else {
							Some(view! {
								<>
									<h2>"Unaffiliated Server"</h2>
									<p class="help-text">"This server is not affiliated with a central server and needs an administrator to finish configuring it."</p>
								</>
							}.into_any())
				}}}
			</Suspense>
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
