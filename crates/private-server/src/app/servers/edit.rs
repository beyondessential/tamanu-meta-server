use commons_errors::AppError;
use commons_types::{
	Uuid,
	geo::GeoPoint,
	server::{kind::ServerKind, rank::ServerRank},
};
use leptos::leptos_dom::helpers::request_animation_frame;
use leptos::prelude::*;
use leptos_router::{components::A, hooks::use_params_map};

use crate::{
	app::servers::geo::CloudRegion,
	components::{ErrorHandler, LoadingBar},
	fns::servers::{ServerDataUpdate, ServerInfo, get_info, search_parent, update},
};

#[component]
pub fn Edit() -> impl IntoView {
	let params = use_params_map();
	let server_id = move || {
		params
			.read()
			.get("id")
			.and_then(|id| id.parse().ok())
			.unwrap_or_default()
	};

	let data = Resource::new(server_id, async move |id| get_info(id).await);

	view! {
		<Transition fallback=|| { view!{ <LoadingBar /> } }>
			<ErrorHandler>
				{move || data.and_then(|info| {
					let info = info.clone();
					view! { <EditForm info /> }
				})}
			</ErrorHandler>
		</Transition>
	}
}

#[component]
pub fn EditForm(info: ServerInfo) -> impl IntoView {
	let server_id = info.id;
	let (name, set_name) = signal(info.name.clone());
	let (host, set_host) = signal(info.host.clone());
	let (kind, set_kind) = signal(info.kind);
	let (rank, set_rank) = signal(info.rank);
	let (listed, set_listed) = signal(info.listed);
	let (parent_id, set_parent_id) = signal(info.parent_server_id);

	let (cloud, set_cloud) = signal(info.cloud);
	let (lat, set_lat) = signal(info.geolocation.map(|geo| geo.lat));
	let (lon, set_lon) = signal(info.geolocation.map(|geo| geo.lon));

	let save_data = Signal::derive(move || ServerDataUpdate {
		name: name.get(),
		host: Some(host.get()),
		kind: Some(kind.get()),
		rank: rank.get(),
		listed: Some(listed.get()),
		parent_server_id: Some(parent_id.get()),
		cloud: Some(cloud.get()),
		geolocation: Some(match (lat.get(), lon.get()) {
			(Some(lat), Some(lon)) => Some(GeoPoint { lat, lon }),
			_ => None,
		}),
		..Default::default()
	});

	let navigate = leptos_router::hooks::use_navigate();
	let submit = Action::new(move |sig: &Signal<ServerDataUpdate>| {
		let data = sig.get();
		let navigate = navigate.clone();
		async move {
			update(server_id, data).await?;
			navigate(&format!("/servers/{server_id}"), Default::default());
			Ok::<_, AppError>(())
		}
	});

	view! {
		<form class="box" on:submit=move |ev| {
			ev.prevent_default();
			submit.dispatch(save_data);
		}>
			<div class="field is-horizontal">
				<div class="field-label is-normal">
					<label class="label" for="field-name">"Name"</label>
				</div>
				<div class="field-body">
					<div class="field">
						<div class="control">
							<input
								id="field-name"
								name="name"
								class="input"
								type="text"
								disabled=move || submit.pending().get()
								prop:value=move || name.get()
								on:change=move |ev| set_name.set(none_if_empty(event_target_value(&ev))) />
						</div>
					</div>
				</div>
			</div>
			<div class="field is-horizontal">
				<div class="field-label is-normal">
					<label class="label" for="field-host">URL</label>
				</div>
				<div class="field-body">
					<div class="field">
						<div class="control">
							<input
								id="field-host"
								name="host"
								class="input"
								type="text"
								disabled=move || submit.pending().get()
								prop:value=move || host.get()
								on:change=move |ev| set_host.set(event_target_value(&ev)) />
						</div>
					</div>
				</div>
			</div>
			<div class="field is-horizontal">
				<div class="field-label is-normal">
					<label class="label" for="field-kind">"Kind"</label>
				</div>
				<div class="field-body">
					<div class="field">
						<div class="control">
							<div class="select">
								<select
									id="field-kind"
									disabled=move || submit.pending().get()
									prop:value=move || kind.get()
									on:change=move |ev| set_kind.set(event_target_value(&ev).parse().unwrap_or_default())
								>
									<option value={ServerKind::Central}>{ServerKind::Central}</option>
									<option value={ServerKind::Facility}>{ServerKind::Facility}</option>
								</select>
							</div>
						</div>
					</div>
				</div>
			</div>
			<div class="field is-horizontal">
				<div class="field-label is-normal">
					<label class="label" for="field-rank">"Rank"</label>
				</div>
				<div class="field-body">
					<div class="field">
						<div class="control">
							<div class="select">
								<select
									id="field-rank"
									disabled=move || submit.pending().get()
									prop:value=move || rank.get().map_or("".to_string(), |rank| rank.to_string())
									on:change=move |ev| set_rank.set(event_target_value(&ev).parse().ok())
								>
									<option value="">"unranked"</option>
									<option value={ServerRank::Production}>{ServerRank::Production}</option>
									<option value={ServerRank::Clone}>{ServerRank::Clone}</option>
									<option value={ServerRank::Demo}>{ServerRank::Demo}</option>
									<option value={ServerRank::Test}>{ServerRank::Test}</option>
									<option value={ServerRank::Dev}>{ServerRank::Dev}</option>
								</select>
							</div>
						</div>
					</div>
				</div>
			</div>
			<div class="field is-horizontal">
				<div class="field-label is-normal">
					<label class="label" for="field-parent-id">"Parent Server"</label>
				</div>
				<div class="field-body">
					<div class="field">
						<ParentServerControl server_id kind rank parent_id set_parent_id pending=submit.pending() />
					</div>
				</div>
			</div>
			<GeolocationControl cloud set_cloud lat set_lat lon set_lon pending=submit.pending() />
			{move || (kind.get() == ServerKind::Central).then(|| view! {
				<div class="field is-horizontal">
					<div class="field-label"></div>
					<div class="field-body">
						<div class="field">
							<div class="control">
								<label class="checkbox">
									<input
										class="mr-2"
										type="checkbox"
										disabled=move || submit.pending().get()
										prop:checked=move || listed.get()
										on:change=move |ev| set_listed.set(event_target_checked(&ev)) />
									"Available in Tamanu Mobile app"
								</label>
							</div>
						</div>
					</div>
				</div>
			})}
			<div class="field is-horizontal">
				<div class="field-label"></div>
				<div class="field-body">
					<div class="field is-grouped">
						<div class="control">
							<button
								type="submit"
								class="button is-primary"
								disabled=move || submit.pending().get()
							>"Save"</button>
						</div>
						<div class="control">
							<A
								href=format!("/servers/{server_id}")
								{..}
								class="button is-danger is-light"
								class:is-disabled=move || submit.pending().get()
							>"Cancel"</A>
						</div>
					</div>
				</div>
			</div>
		</form>
	}
}

#[component]
pub fn GeolocationControl(
	cloud: ReadSignal<Option<bool>>,
	set_cloud: WriteSignal<Option<bool>>,
	lat: ReadSignal<Option<f64>>,
	set_lat: WriteSignal<Option<f64>>,
	lon: ReadSignal<Option<f64>>,
	set_lon: WriteSignal<Option<f64>>,
	pending: Memo<bool>,
) -> impl IntoView {
	view! {
		<div class="field is-horizontal">
			<div class="field-label is-normal">
				<label class="label" for="field-cloud">"Location"</label>
			</div>
			<div class="field-body">
				<div class="field is-narrow">
					<div class="control is-expanded">
						<div class="select is-fullwidth">
							<select
								id="field-cloud"
								disabled=move || pending.get()
								prop:value=move || cloud.get().map_or("unknown", |cloud| if cloud { "true" } else { "false" })
								on:change=move |ev| set_cloud.set(match event_target_value(&ev).as_str() {
									"true" => Some(true),
									"false" => Some(false),
									_ => None,
								})
							>
								<option value="unknown">"unknown"</option>
								<option value="true">"cloud"</option>
								<option value="false">"on premise"</option>
							</select>
						</div>
					</div>
				</div>
				{move || (cloud.get() == Some(true)).then(|| view! { <RegionSelection lat set_lat lon set_lon pending /> })}
				<div class="field">
					<div class="control is-expanded">
						<input
							class="input"
							type="text"
							placeholder="Latitude"
							disabled=move || pending.get()
							prop:value=move || lat.get().map_or(String::new(), |n| n.to_string())
							on:change=move |ev| set_lat.set(event_target_value(&ev).parse().ok()) />
					</div>
				</div>
				<div class="field">
					<div class="control is-expanded">
						<input
							class="input"
							type="text"
							placeholder="Longitude"
							disabled=move || pending.get()
							prop:value=move || lon.get().map_or(String::new(), |n| n.to_string())
							on:change=move |ev| set_lon.set(event_target_value(&ev).parse().ok()) />
					</div>
				</div>
			</div>
		</div>
	}
}

#[component]
pub fn RegionSelection(
	lat: ReadSignal<Option<f64>>,
	set_lat: WriteSignal<Option<f64>>,
	lon: ReadSignal<Option<f64>>,
	set_lon: WriteSignal<Option<f64>>,
	pending: Memo<bool>,
) -> impl IntoView {
	let (region, set_region) = signal(None::<CloudRegion>);

	Effect::new(move || {
		if let (Some(lat), Some(lon)) = (lat.get(), lon.get()) {
			set_region.set(CloudRegion::from_lat_lon(lat, lon));
		}
	});

	view! {
		<div class="field is-narrow">
			<div class="control is-expanded">
				<div class="select is-fullwidth">
					<select
						disabled=move || pending.get()
						prop:value=move || region.get().map_or("", |reg| reg.as_str())
						on:change=move |ev| {
							let region = event_target_value(&ev).parse().ok();
							set_region.set(region);
							match region.map(|reg| reg.to_lat_lon()) {
								Some((lat, lon)) => {
									set_lat.set(Some(lat));
									set_lon.set(Some(lon));
								}
								None => {
									set_lat.set(None);
									set_lon.set(None);
								}
							}
						}
					>
						<option disabled value="">"Other region"</option>
						<For each=move || CloudRegion::ALL key=|r| *r let:region>
							<option value={region.as_str()}>{region.as_str()}</option>
						</For>
					</select>
				</div>
			</div>
		</div>
	}
}

#[component]
pub fn ParentServerControl(
	server_id: Uuid,
	kind: ReadSignal<ServerKind>,
	rank: ReadSignal<Option<ServerRank>>,
	parent_id: ReadSignal<Option<Uuid>>,
	set_parent_id: WriteSignal<Option<Uuid>>,
	pending: Memo<bool>,
) -> impl IntoView {
	let (parent_search_query, set_parent_search_query) = signal(String::new());
	let (show_parent_results, set_show_parent_results) = signal(false);

	let current_parent_info = Resource::new(
		move || parent_id.get(),
		move |id| async move {
			if let Some(parent_id) = id {
				get_info(parent_id).await.ok()
			} else {
				None
			}
		},
	);

	let parent_search_results = Resource::new(
		move || (parent_search_query.get(), kind.get(), rank.get()),
		move |(query, current_kind, current_rank)| async move {
			if query.is_empty() {
				return Ok::<Vec<ServerInfo>, AppError>(Vec::new());
			}
			search_parent(query, server_id, current_rank, current_kind).await
		},
	);

	view! {
		<div class="control">
			<input
				id="field-parent-id"
				name="parent-id"
				class="input"
				type="text"
				placeholder="Enter UUID or search by name/host"
				disabled=move || pending.get()
				prop:value=move || {
					parent_id.get()
						.map(|id| id.to_string())
						.unwrap_or_else(|| parent_search_query.get())
				}
				on:input=move |ev| {
					let value = event_target_value(&ev);
					if let Ok(uuid) = value.parse::<Uuid>() {
						set_parent_id.set(Some(uuid));
						set_show_parent_results.set(false);
					} else {
						set_parent_search_query.set(value);
						set_show_parent_results.set(true);
					}
				}
				on:focus=move |_| {
					if !parent_search_query.get().is_empty() {
						set_show_parent_results.set(true);
					}
				}
				on:blur=move |_| {
					request_animation_frame(move || {
						set_show_parent_results.set(false);
					});
				}
			/>
			{move || {
				parent_id.get().map(|_| {
					view! {
						<div class="mt-2" style="display: flex; align-items: center; gap: 1rem;">
							<Suspense fallback=move || view! { <span class="tag">"Loading..."</span> }>
								{move || {
									current_parent_info.get().flatten().map(|current| {
										let display_name = current.name.clone()
											.unwrap_or_else(|| current.host.clone());
										let rank_text = current.rank.map(|r| r.to_string()).unwrap_or_else(|| "unranked".to_string());
										let kind_text = current.kind.to_string();
										view! {
											<span class="tag is-info is-light">
												"Current: " {display_name} " (" {kind_text} ", " {rank_text} ")"
											</span>
										}
									})
								}}
							</Suspense>
							<button
								type="button"
								class="button is-small"
								on:click=move |_| {
									set_parent_id.set(None);
									set_parent_search_query.set(String::new());
								}
							>
								"Clear parent"
							</button>
						</div>
					}
				})
			}}
		</div>
		<Transition>
			{move || {
				(show_parent_results.get() && !parent_search_query.get().is_empty()).then(|| {
					view! {
						<div class="dropdown is-active" style="width: 100%; position: relative;">
							<div class="dropdown-menu" style="width: 100%; position: absolute; top: 0; left: 0;">
								<div class="dropdown-content">
									<Suspense fallback=move || view! { <div class="dropdown-item">"Loading..."</div> }>
										{move || {
											let current_parent = current_parent_info.get().flatten();
											let current_parent_id = parent_id.get();

											parent_search_results.and_then(|results| {
												let mut items = vec![];

												if let Some(ref current) = current_parent {
													let server_id_val = current.id;
													let display_name = current.name.clone()
														.unwrap_or_else(|| current.host.clone());
													let rank_text = current.rank.map(|r| r.to_string()).unwrap_or_else(|| "unranked".to_string());
													let kind_text = current.kind.to_string();

													items.push(view! {
														<a
															class="dropdown-item"
															style="cursor: pointer;"
															on:mousedown=move |ev| {
																ev.prevent_default();
																set_parent_id.set(Some(server_id_val));
																set_parent_search_query.set(String::new());
																set_show_parent_results.set(false);
															}
														>
															<div>
																<strong>
																	"✓ "
																	{display_name}
																</strong>
																<br/>
																<small>{current.host.clone()} " • " {kind_text} " • " {rank_text}</small>
															</div>
														</a>
													}.into_any());

													if !results.is_empty() {
														items.push(view! {
															<hr class="dropdown-divider" />
														}.into_any());
													}
												}

												if results.is_empty() && current_parent.is_none() {
													items.push(view! {
														<div class="dropdown-item">"No results found"</div>
													}.into_any());
												} else {
													for server in results.iter() {
														if Some(server.id) == current_parent_id {
															continue;
														}

														let server_id_val = server.id;
														let display_name = server.name.clone()
															.unwrap_or_else(|| server.host.clone());
														let rank_text = server.rank.map(|r| r.to_string()).unwrap_or_else(|| "unranked".to_string());
														let kind_text = server.kind.to_string();
														items.push(view! {
															<a
																class="dropdown-item"
																style="cursor: pointer;"
																on:mousedown=move |ev| {
																	ev.prevent_default();
																	set_parent_id.set(Some(server_id_val));
																	set_parent_search_query.set(String::new());
																	set_show_parent_results.set(false);
																}
															>
																<div>
																	<strong>{display_name}</strong>
																	<br/>
																	<small>{server.host.clone()} " • " {kind_text} " • " {rank_text}</small>
																</div>
															</a>
														}.into_any());
													}
												}

												items.into_any()
											})
										}}
									</Suspense>
								</div>
							</div>
						</div>
					}
				})
			}}
		</Transition>
	}
}

fn none_if_empty(s: String) -> Option<String> {
	if s.is_empty() { None } else { Some(s) }
}
