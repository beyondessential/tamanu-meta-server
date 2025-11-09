use commons_types::{
	Uuid,
	server::{kind::ServerKind, rank::ServerRank},
	status::ShortStatus,
};
use leptos::prelude::*;
use leptos::serde_json;
use leptos_meta::Stylesheet;
use leptos_router::components::A;
use leptos_router::components::Redirect;
use leptos_router::hooks::use_params_map;

use crate::app::devices::DeviceListItem;
use crate::components::{StatusLegend, TimeAgo, VersionIndicator, VersionLegend};
use crate::fns::devices::DeviceInfo;
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

	let is_admin = is_admin_resource();

	let update_action = Action::new(
		move |(name, host, rank, device_id, parent_id): &(
			Option<String>,
			Option<String>,
			Option<ServerRank>,
			Option<Uuid>,
			Option<Uuid>,
		)| {
			let name = name.clone();
			let host = host.clone();
			let id = server_id();
			let rank = *rank;
			let device_id = *device_id;
			let parent_id = *parent_id;
			async move {
				let result = update_server(id, name, host, rank, device_id, parent_id).await;
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
										// Check if user is admin before redirecting
										if !is_admin.get().unwrap_or(false) {
											return view! {
												<Redirect path={format!("/servers/{}", parent_id)} />
											}.into_any();
										}
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
										update_action
										server_id=server_id()
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
	update_action: Action<
		(
			Option<String>,
			Option<String>,
			Option<ServerRank>,
			Option<Uuid>,
			Option<Uuid>,
		),
		Result<crate::fns::servers::ServerDetailsData, commons_errors::AppError>,
	>,
	server_id: Uuid,
) -> impl IntoView {
	let server = data.server.clone();
	let device_info = data.device_info.clone();
	let last_status = data.last_status.clone();
	let up = data.up.clone();
	let child_servers = data.child_servers.clone();

	let server_name = server.name.clone();
	let server_host = server.host.clone();
	let server_rank = server.rank;
	let server_kind = server.kind;
	let device_id = device_info.as_ref().map(|d| d.device.id);

	let server_name_for_effect = server_name.clone();
	let server_host_for_effect = server_host.clone();

	Effect::new(move |_| {
		if !is_editing.get() {
			edit_name.set(server_name_for_effect.clone());
			edit_host.set(server_host_for_effect.clone());
			edit_rank.set(server_rank);
			edit_device_id.set(device_id);
			edit_parent_id.set(server.parent_server_id);
		}
	});

	view! {
		<div class="detail-container">
			<PageHeader
				server_name=server_name.clone()
				server_host=server_host.clone()
				server_rank=server.rank
				server_kind=server_kind
				up=up
				device_id=device_id
				child_servers=child_servers.clone()
				is_editing=is_editing
				edit_name=edit_name
				edit_host=edit_host
				edit_rank=edit_rank
				edit_device_id=edit_device_id
				_edit_parent_id=edit_parent_id
			/>

			{move || {
				if is_editing.get() {
					view! {
						<EditForm
							edit_name=edit_name
							edit_host=edit_host
							edit_rank=edit_rank
							edit_device_id=edit_device_id
							edit_parent_id=edit_parent_id
							update_action=update_action
							is_editing=is_editing
							server_kind=server_kind
						/>
					}.into_any()
				} else {
					view! {
						<>
							<ServerInfoSection
								host=server.host.clone()
								device_info=device_info.clone()
								up=up.to_string()
							/>
							{last_status.as_ref().map(|status| {
								view! {
									<StatusSection status=status.clone() />
								}
							})}
							{if server.kind == ServerKind::Central && !data.child_servers.is_empty() {
								view! {
									<ChildServersSection child_servers=data.child_servers.clone() is_admin />
								}.into_any()
							} else if server.kind != ServerKind::Central && server.parent_server_id.is_none() {
								view! {
									<AssignParentSection server_id=server_id.clone() />
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
	server_name: String,
	server_host: String,
	server_rank: Option<ServerRank>,
	server_kind: ServerKind,
	up: ShortStatus,
	device_id: Option<Uuid>,
	child_servers: Vec<ChildServerData>,
	is_editing: RwSignal<bool>,
	edit_name: RwSignal<String>,
	edit_host: RwSignal<String>,
	edit_rank: RwSignal<Option<ServerRank>>,
	edit_device_id: RwSignal<Option<Uuid>>,
	_edit_parent_id: RwSignal<Option<Uuid>>,
) -> impl IntoView {
	let name_clone = server_name.clone();
	let host_clone = server_host.clone();

	let is_admin = is_admin_resource();

	view! {
		<div class="page-header">
			<Suspense>
				{move || {
					if is_admin.get().unwrap_or(false) && !is_editing.get() {
						let name = name_clone.clone();
						let host = host_clone.clone();
						Some(view! {
							<button
								class="edit-button"
								on:click=move |_| {
									edit_name.set(name.clone());
									edit_host.set(host.clone());
									edit_rank.set(server_rank);
									edit_device_id.set(device_id);
									// edit_parent_id already set by Effect
									is_editing.set(true);
								}
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
				<span class={format!("status-dot {up}")} title={format!("{server_name}: {up}")}></span>
				{if server_kind == ServerKind::Central {
					child_servers.into_iter().map(|child| {
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
				{server_name.clone()}
			</h1>
			<span class="server-rank">{server_rank.map(|r| r.to_string()).unwrap_or_default()}</span>
		</div>
	}
}

#[component]
fn EditForm(
	edit_name: RwSignal<String>,
	edit_host: RwSignal<String>,
	edit_rank: RwSignal<Option<ServerRank>>,
	edit_device_id: RwSignal<Option<Uuid>>,
	edit_parent_id: RwSignal<Option<Uuid>>,
	update_action: Action<
		(
			Option<String>,
			Option<String>,
			Option<ServerRank>,
			Option<Uuid>,
			Option<Uuid>,
		),
		Result<crate::fns::servers::ServerDetailsData, commons_errors::AppError>,
	>,
	is_editing: RwSignal<bool>,
	server_kind: ServerKind,
) -> impl IntoView {
	view! {
		<section class="detail-section edit-form">
			<h2>"Edit Server Details"</h2>
			<form on:submit=move |ev| {
				ev.prevent_default();
				let device_id = edit_device_id.get();
				let parent_id = edit_parent_id.get();
				update_action.dispatch((
					Some(edit_name.get()),
					Some(edit_host.get()),
					edit_rank.get(),
					device_id,
					parent_id,
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

				{if server_kind != ServerKind::Central {
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
					().into_any()
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
fn ServerInfoSection(host: String, device_info: Option<DeviceInfo>, up: String) -> impl IntoView {
	let host_clone = host.clone();
	view! {
		<section class="detail-section">
			<h2>
				<span class={format!("section-status-dot status-dot {up}")}></span>
				"Central server"
				<a class="detail-host" href={host} target="_blank">{host_clone}</a>
			</h2>
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
fn ChildServersSection(
	child_servers: Vec<ChildServerData>,
	is_admin: Resource<bool>,
) -> impl IntoView {
	view! {
		<section class="detail-section">
			<h2>"Facility servers (" {child_servers.len()} ")"</h2>
			<div class="child-servers-list">
				{child_servers.into_iter().map(|child| {
					view! {
						<ChildServerCard child is_admin />
					}
				}).collect::<Vec<_>>()}
			</div>
		</section>
	}
}

#[component]
fn ChildServerCard(child: ChildServerData, is_admin: Resource<bool>) -> impl IntoView {
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
								<A class:edit-button href={format!("/servers/{}/edit", child.id)}>
									"Edit"
								</A>
							})
						} else {
							None
						}
					}}
				</Suspense>
			</div>
			<div>
				<a class="detail-host" href={child.host.clone()} target="_blank">{child.host.clone()}</a>
			</div>
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
			{child.device_info.as_ref().map(|device_info| {
				let device_info = device_info.clone();
				view! {
					<div class="child-server-device">
						<h4>"Device"</h4>
						<DeviceListItem device=device_info class:vertical class:hide-id />
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
