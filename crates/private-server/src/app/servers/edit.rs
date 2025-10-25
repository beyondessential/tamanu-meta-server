use commons_types::{Uuid, server::rank::ServerRank};
use leptos::prelude::*;
use leptos_meta::Stylesheet;
use leptos_router::components::Redirect;
use leptos_router::hooks::use_params_map;

use crate::fns::servers::{
	ServerListItem, assign_parent_server, search_central_servers, server_detail, update_server,
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

	let search_query = RwSignal::new(String::new());
	let search_results = RwSignal::new(Vec::<ServerListItem>::new());

	let is_admin = Resource::new(
		|| (),
		|_| async { crate::fns::commons::is_current_user_admin().await },
	);

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
				let result =
					update_server(id.clone(), name, host, rank, device_id, parent_id).await;
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
									let server = data.server.clone();
									let server_name = server.name.clone();
									let server_host = server.host.clone();
									let server_rank = server.rank;
									let device_id = data.device_info
										.as_ref()
										.map(|d| d.device.id);
									let current_rank = server.rank;

									Effect::new(move |_| {
										edit_name.set(server_name.clone());
										edit_host.set(server_host.clone());
										edit_rank.set(server_rank);
										edit_device_id.set(device_id);
										edit_parent_id.set(server.parent_server_id);
									});

									view! {
										<EditView
											server=data.server.clone()
											edit_name=edit_name
											edit_host=edit_host
											edit_rank=edit_rank
											edit_device_id=edit_device_id
											edit_parent_id=edit_parent_id
											update_action=update_action
											search_query=search_query
											search_results=search_results
											search_action=search_action
											assign_action=assign_action
											current_rank=current_rank
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
	server: crate::fns::servers::ServerDetailsData,
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
	search_query: RwSignal<String>,
	search_results: RwSignal<Vec<ServerListItem>>,
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
