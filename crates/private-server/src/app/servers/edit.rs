use std::sync::Arc;

use commons_types::{
	Uuid,
	geo::GeoPoint,
	server::{kind::ServerKind, rank::ServerRank},
};
use leptos::prelude::*;
use leptos_meta::Stylesheet;
use leptos_router::components::Redirect;
use leptos_router::hooks::use_params_map;

use crate::fns::servers::{
	ServerDetailData, ServerInfo, assign_parent_server, search_central_servers, server_detail,
	update_server,
};

#[component]
pub fn Edit() -> impl IntoView {
	let params = use_params_map();
	let server_id = move || {
		params
			.read()
			.get("id")
			.map(|id| id.parse().ok())
			.flatten()
			.unwrap_or_default()
	};

	let detail_resource =
		Resource::new(move || server_id(), async move |id| server_detail(id).await);

	let edit_name = RwSignal::new(String::new());
	let edit_host = RwSignal::new(String::new());
	let edit_rank = RwSignal::new(None::<ServerRank>);
	let edit_device_id = RwSignal::new(None::<Uuid>);
	let edit_parent_id = RwSignal::new(None::<Uuid>);
	let edit_cloud = RwSignal::new(None::<Option<bool>>);
	let edit_lat = RwSignal::new(None::<f64>);
	let edit_lon = RwSignal::new(None::<f64>);
	let edit_aws_region = RwSignal::new(String::new());

	let search_query = RwSignal::new(String::new());
	let search_results = RwSignal::new(Vec::<ServerInfo>::new());

	let is_admin = Resource::new(
		|| (),
		|_| async { crate::fns::commons::is_current_user_admin().await },
	);

	let update_action = Action::new(
		move |(name, host, rank, device_id, parent_id, cloud, geolocation): &(
			Option<String>,
			Option<String>,
			Option<ServerRank>,
			Option<Uuid>,
			Option<Uuid>,
			Option<Option<bool>>,
			Option<Option<GeoPoint>>,
		)| {
			let name = name.clone();
			let host = host.clone();
			let id = server_id();
			let rank = *rank;
			let device_id = *device_id;
			let parent_id = *parent_id;
			let cloud = *cloud;
			let geolocation = geolocation.clone();
			async move {
				let result = update_server(
					id.clone(),
					name,
					host,
					rank,
					device_id,
					parent_id,
					None,
					cloud,
					geolocation,
				)
				.await;
				if result.is_ok() {
					leptos_router::hooks::use_navigate()(
						&format!("/servers/{}", id),
						Default::default(),
					);
				}
				result
			}
		},
	);

	let assign_action = Action::new(move |parent_id: &Uuid| {
		let server_id = server_id();
		let parent_id = *parent_id;
		async move {
			let result = assign_parent_server(server_id, parent_id).await;
			if result.is_ok() {
				edit_parent_id.set(Some(parent_id));
				search_query.set(String::new());
				search_results.set(Vec::new());
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
		<Stylesheet id="css-servers" href="/static/servers.css" />
		<div id="server-edit-page">
			<Suspense fallback=|| view! { <div class="loading">"Loading server details..."</div> }>
				{move || {
					is_admin.get().and_then(|result| {
						if !result.ok().unwrap_or(false) {
							return Some(view! { <Redirect path="/servers" /> }.into_any());
						}

						detail_resource.get().map(|result| {
							match result {
								Ok(data) => {
									Effect::new({ let data = data.clone(); move |_| {
										edit_name.set(data.server.name.clone());
										edit_host.set(data.server.host.clone());
										edit_rank.set(data.server.rank);
										edit_device_id.set(data.device_info
											.as_ref()
											.map(|d| d.device.id));
										edit_parent_id.set(data.server.parent_server_id);
										edit_cloud.set(Some(data.server.cloud));
										if let Some(geo) = &data.server.geolocation {
											edit_lat.set(Some(geo.lat));
											edit_lon.set(Some(geo.lon));
										}
									}});

									view! {
										<EditView
											server=data.server.clone()
											edit_name=edit_name
											edit_host=edit_host
											edit_rank=edit_rank
											edit_device_id=edit_device_id
											edit_parent_id=edit_parent_id
											edit_cloud=edit_cloud
											edit_lat=edit_lat
											edit_lon=edit_lon
											edit_aws_region=edit_aws_region
											update_action=update_action
											search_query=search_query
											search_results=search_results
											search_action=search_action
											assign_action=assign_action
											current_rank=data.server.rank
										/>
									}.into_any()
								}
								Err(e) => {
									view! {
										<div class="error-message">
											{format!("Failed to load server: {}", e)}
										</div>
									}.into_any()
								}
							}
						})
					})
				}}
			</Suspense>
		</div>
	}
}

#[component]
fn EditView(
	server: Arc<crate::fns::servers::ServerDetailsData>,
	edit_name: RwSignal<String>,
	edit_host: RwSignal<String>,
	edit_rank: RwSignal<Option<ServerRank>>,
	edit_device_id: RwSignal<Option<Uuid>>,
	edit_parent_id: RwSignal<Option<Uuid>>,
	edit_cloud: RwSignal<Option<Option<bool>>>,
	edit_lat: RwSignal<Option<f64>>,
	edit_lon: RwSignal<Option<f64>>,
	edit_aws_region: RwSignal<String>,
	update_action: Action<
		(
			Option<String>,
			Option<String>,
			Option<ServerRank>,
			Option<Uuid>,
			Option<Uuid>,
			Option<Option<bool>>,
			Option<Option<GeoPoint>>,
		),
		Result<crate::fns::servers::ServerDetailsData, commons_errors::AppError>,
	>,
	search_query: RwSignal<String>,
	search_results: RwSignal<Vec<ServerInfo>>,
	search_action: Action<String, Result<(), commons_errors::AppError>>,
	assign_action: Action<
		Uuid,
		Result<crate::fns::servers::ServerDetailsData, commons_errors::AppError>,
	>,
	current_rank: Option<ServerRank>,
) -> impl IntoView {
	view! {
		<div class="detail-container">
			<div class="page-header">
			{if let Some(parent_id) = server.parent_server_id.as_ref() {
				view! { <a href={format!("/servers/{parent_id}")} class="back-link">"← Back to central"</a> }
			} else {
				view! { <a href="/servers/facilities".to_string() class="back-link">"← Back to list"</a> }
			}}
				<h1>"Edit " {server.name.clone()}</h1>
			</div>

			<section class="detail-section">
				<h2>"Assign to central"</h2>
				{server.parent_server_id.as_ref().map(|parent_id| {
					let parent_name = server.parent_server_name.clone().unwrap_or_else(|| "(unnamed)".to_string());
					let parent_id = parent_id.clone();
					view! {
						<div class="current-parent">
							<span class="info-label">"Currently assigned to: "</span>
							<a href={format!("/servers/{parent_id}")} class="parent-link">
								{parent_name}
							</a>
						</div>
					}
				})}
				<p class="help-text">"Search and select a central server to change the parent assignment."</p>
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
							let mut results = search_results.get();

							// Sort: matching rank first, then others
							results.sort_by(|a, b| {
								let a_matches = a.rank == current_rank;
								let b_matches = b.rank == current_rank;
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
										let rank_matches = server.rank == current_rank;
										let opacity_class = if rank_matches { "" } else { "faded" };
										view! {
											<div class={format!("search-result-item {}", opacity_class)}>
												<div class="search-result-info">
													<strong>{server.name.unwrap_or_else(|| "(unnamed)".to_string())}</strong>
													<span class="search-result-host">{server.host}</span>
													{server.rank.map(|rank| {
														view! {
															<span class="search-result-rank">{rank}</span>
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
								Some(view! {
									<div class="success-message">
										{"Parent server assigned successfully".to_string()}
									</div>
								})
							}
						})
					}}
				</div>
			</section>

			<section class="detail-section edit-form">
				<h2>"Server Details"</h2>
				<form on:submit=move |ev| {
					ev.prevent_default();
					let device_id = edit_device_id.get();
					let parent_id = edit_parent_id.get();
					let cloud = edit_cloud.get();
					let lat = edit_lat.get();
					let lon = edit_lon.get();
					let geolocation = if let (Some(lat), Some(lon)) = (lat, lon) {
						Some(Some(GeoPoint { lat, lon }))
					} else {
						Some(None)
					};
					update_action.dispatch((
						Some(edit_name.get()),
						Some(edit_host.get()),
						edit_rank.get(),
						device_id,
						parent_id,
						cloud,
						geolocation,
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
							prop:value=move || edit_rank.get().unwrap_or_default()
							on:change=move |ev| edit_rank.set(event_target_value(&ev).parse().ok())
							required
						>
							<option value={ServerRank::Production} selected=move || edit_rank.get() == Some(ServerRank::Production)>{ServerRank::Production}</option>
							<option value={ServerRank::Clone} selected=move || edit_rank.get() == Some(ServerRank::Clone)>{ServerRank::Clone}</option>
							<option value={ServerRank::Demo} selected=move || edit_rank.get() == Some(ServerRank::Demo)>{ServerRank::Demo}</option>
							<option value={ServerRank::Test} selected=move || edit_rank.get() == Some(ServerRank::Test)>{ServerRank::Test}</option>
							<option value={ServerRank::Dev} selected=move || edit_rank.get() == Some(ServerRank::Dev)>{ServerRank::Dev}</option>
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

					<div class="form-group">
						<label for="edit-cloud">"Server location"</label>
						<select
							id="edit-cloud"
							prop:value=move || {
								match edit_cloud.get() {
									Some(Some(true)) => "cloud",
									Some(Some(false)) => "on-premise",
									_ => "unknown",
								}.to_string()
							}
							on:change=move |ev| {
								let value = event_target_value(&ev);
								edit_cloud.set(Some(match value.as_str() {
									"cloud" => Some(true),
									"on-premise" => Some(false),
									_ => None,
								}));
								edit_aws_region.set(String::new());
							}
						>
							<option value="unknown">"Unknown"</option>
							<option value="cloud">"Cloud"</option>
							<option value="on-premise">"On premise"</option>
						</select>
					</div>

					{move || {
						edit_cloud.get().flatten().and_then(|is_cloud| is_cloud.then(|| {
							view! {
								<div class="form-group">
									<label for="edit-aws-region">"AWS Region"</label>
									<select
										id="edit-aws-region"
										prop:value=move || edit_aws_region.get()
										on:change=move |ev| {
											let region = event_target_value(&ev);
											edit_aws_region.set(region.clone());
											let (lat, lon) = match region.as_str() {
												"sydney" => (-33.8688, 151.2093),
												"auckland" => (-37.0082, 174.7850),
												"singapore" => (1.3521, 103.8198),
												"tokyo" => (35.6762, 139.6503),
												"zurich" => (47.3769, 8.5472),
												"mumbai" => (19.0760, 72.8777),
												_ => return,
											};
											edit_lat.set(Some(lat));
											edit_lon.set(Some(lon));
										}
									>
										<option value="">"Select a region..."</option>
										<option value="sydney">"AWS Sydney"</option>
										<option value="auckland">"AWS Auckland"</option>
										<option value="singapore">"AWS Singapore"</option>
										<option value="tokyo">"AWS Tokyo"</option>
										<option value="zurich">"AWS Zurich"</option>
										<option value="mumbai">"AWS Mumbai"</option>
									</select>
									<small class="help-text">"Select an AWS region to auto-populate coordinates"</small>
								</div>
							}
						}))
					}}

					{move || {
						edit_cloud.get().flatten().map(|_| {
							view! {
								<div class="form-group">
									<label>"Geolocation Coordinates"</label>
									<div style="display: flex; gap: 1rem;">
										<div style="flex: 1;">
											<label for="edit-lat" style="display: block; font-size: 0.9em; margin-bottom: 0.25rem;">"Latitude"</label>
											<input
												type="number"
												id="edit-lat"
												step="any"
												prop:value=move || edit_lat.get()
												on:input=move |ev| edit_lat.set(event_target_value(&ev).parse().ok())
												placeholder="e.g., -33.8688"
											/>
										</div>
										<div style="flex: 1;">
											<label for="edit-lon" style="display: block; font-size: 0.9em; margin-bottom: 0.25rem;">"Longitude"</label>
											<input
												type="number"
												id="edit-lon"
												step="any"
												prop:value=move || edit_lon.get()
												on:input=move |ev| edit_lon.set(event_target_value(&ev).parse().ok())
												placeholder="e.g., 151.2093"
											/>
										</div>
									</div>
									<small class="help-text">"Optional latitude and longitude coordinates"</small>
								</div>
							}
						})
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
							{move || if update_action.pending().get() { "Saving..." } else { "Save Changes" }}
						</button>
					</div>
				</form>
			</section>
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
			Option<Option<bool>>,
			Option<Option<GeoPoint>>,
		),
		Result<crate::fns::servers::ServerDetailsData, commons_errors::AppError>,
	>,
	is_editing: RwSignal<bool>,
) -> impl IntoView {
	let edit_cloud = RwSignal::new(None::<Option<bool>>);
	let edit_lat = RwSignal::new(None::<f64>);
	let edit_lon = RwSignal::new(None::<f64>);
	let edit_aws_region = RwSignal::new(String::new());

	Effect::new({
		let data = data.clone();
		move |_| {
			edit_cloud.set(Some(data.server.cloud));
			if let Some(geo) = &data.server.geolocation {
				edit_lat.set(Some(geo.lat));
				edit_lon.set(Some(geo.lon));
			}
		}
	});

	view! {
		<section class="detail-section edit-form">
			<h2>"Edit Server Details"</h2>
			<form on:submit=move |ev| {
				ev.prevent_default();
				let device_id = edit_device_id.get();
				let parent_id = edit_parent_id.get();
				let listed = edit_listed.get();
				let cloud = edit_cloud.get();
				let lat = edit_lat.get();
				let lon = edit_lon.get();
				let geolocation = if let (Some(lat), Some(lon)) = (lat, lon) {
					Some(Some(GeoPoint { lat, lon }))
				} else {
					Some(None)
				};
				update_action.dispatch((
					Some(edit_name.get()),
					Some(edit_host.get()),
					edit_rank.get(),
					device_id,
					parent_id,
					Some(listed),
					cloud,
					geolocation,
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
						prop:value=move || edit_rank.get().unwrap_or_default()
						on:change=move |ev| edit_rank.set(event_target_value(&ev).parse().ok())
						required
					>
						<option value={ServerRank::Production} selected=move || edit_rank.get() == Some(ServerRank::Production)>{ServerRank::Production}</option>
						<option value={ServerRank::Clone} selected=move || edit_rank.get() == Some(ServerRank::Clone)>{ServerRank::Clone}</option>
						<option value={ServerRank::Demo} selected=move || edit_rank.get() == Some(ServerRank::Demo)>{ServerRank::Demo}</option>
						<option value={ServerRank::Test} selected=move || edit_rank.get() == Some(ServerRank::Test)>{ServerRank::Test}</option>
						<option value={ServerRank::Dev} selected=move || edit_rank.get() == Some(ServerRank::Dev)>{ServerRank::Dev}</option>
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

				<div class="form-group">
					<label for="edit-cloud">"Server location"</label>
					<select
						id="edit-cloud"
						prop:value=move || {
							match edit_cloud.get() {
								Some(Some(true)) => "cloud",
								Some(Some(false)) => "on-premise",
								_ => "unknown",
							}.to_string()
						}
						on:change=move |ev| {
							let value = event_target_value(&ev);
							edit_cloud.set(Some(match value.as_str() {
								"cloud" => Some(true),
								"on-premise" => Some(false),
								_ => None,
							}));
							edit_aws_region.set(String::new());
						}
					>
						<option value="unknown">"Unknown"</option>
						<option value="cloud">"Cloud"</option>
						<option value="on-premise">"On premise"</option>
					</select>
				</div>

				{move || {
					if let Some(Some(true)) = edit_cloud.get() {
						view! {
							<div class="form-group">
								<label for="edit-aws-region">"AWS Region"</label>
								<select
									id="edit-aws-region"
									prop:value=move || edit_aws_region.get()
									on:change=move |ev| {
										let region = event_target_value(&ev);
										edit_aws_region.set(region.clone());
										match region.as_str() {
											"sydney" => {
												edit_lat.set(Some(-33.8688));
												edit_lon.set(Some(151.2093));
											}
											"auckland" => {
												edit_lat.set(Some(-37.0082));
												edit_lon.set(Some(174.7850));
											}
											"singapore" => {
												edit_lat.set(Some(1.3521));
												edit_lon.set(Some(103.8198));
											}
											"tokyo" => {
												edit_lat.set(Some(35.6762));
												edit_lon.set(Some(139.6503));
											}
											"zurich" => {
												edit_lat.set(Some(47.3769));
												edit_lon.set(Some(8.5472));
											}
											"mumbai" => {
												edit_lat.set(Some(19.0760));
												edit_lon.set(Some(72.8777));
											}
											_ => {}
										}
									}
								>
									<option value="">"Select a region..."</option>
									<option value="sydney">"AWS Sydney"</option>
									<option value="auckland">"AWS Auckland"</option>
									<option value="singapore">"AWS Singapore"</option>
									<option value="tokyo">"AWS Tokyo"</option>
									<option value="zurich">"AWS Zurich"</option>
									<option value="mumbai">"AWS Mumbai"</option>
								</select>
								<small class="help-text">"Select an AWS region to auto-populate coordinates"</small>
							</div>
						}.into_any()
					} else {
						().into_any()
					}
				}}

				<div class="form-group">
					<label>"Geolocation Coordinates"</label>
					<div style="display: flex; gap: 1rem;">
						<div style="flex: 1;">
							<label for="edit-lat" style="display: block; font-size: 0.9em; margin-bottom: 0.25rem;">"Latitude"</label>
							<input
								type="number"
								id="edit-lat"
								step="any"
								prop:value=move || edit_lat.get().map(|v| v.to_string()).unwrap_or_default()
								on:input=move |ev| edit_lat.set(event_target_value(&ev).parse().ok())
								placeholder="e.g., -33.8688"
							/>
						</div>
						<div style="flex: 1;">
							<label for="edit-lon" style="display: block; font-size: 0.9em; margin-bottom: 0.25rem;">"Longitude"</label>
							<input
								type="number"
								id="edit-lon"
								step="any"
								prop:value=move || edit_lon.get().map(|v| v.to_string()).unwrap_or_default()
								on:input=move |ev| edit_lon.set(event_target_value(&ev).parse().ok())
								placeholder="e.g., 151.2093"
							/>
						</div>
					</div>
					<small class="help-text">"Optional latitude and longitude coordinates"</small>
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
fn AssignParentSection(server_id: Uuid) -> impl IntoView {
	let search_query = RwSignal::new(String::new());
	let search_results = RwSignal::new(Vec::new());

	let current_rank = RwSignal::new(None::<ServerRank>);

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
												{server.rank.map(|rank| {
													view! {
														<span class="search-result-rank">{rank}</span>
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
		</section>
	}
}
