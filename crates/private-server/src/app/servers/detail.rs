use std::sync::Arc;

use commons_types::{Uuid, geo::GeoPoint, server::kind::ServerKind};
use leptos::{prelude::*, serde_json};
use leptos_meta::Stylesheet;
use leptos_router::{components::A, hooks::use_params_map};

use crate::{
	app::servers::geo::CloudRegion,
	components::{
		DeviceShorty, LoadingBar, ServerKindBadge, ServerRankBadge, ServerShorty, StatusDot,
		StatusLegend, TimeAgo, VersionIndicator, VersionLegend,
	},
	fns::servers::{ServerDetailData, ServerInfo, ServerLastStatusData, get_detail},
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
			.and_then(|id| id.parse::<Uuid>().ok())
			.unwrap_or_default()
	};

	let detail_resource = Resource::new(server_id, async move |id| get_detail(id).await);

	let is_admin = is_admin_resource();

	view! {
		<Stylesheet id="css-servers" href="/static/servers.css" />
		<div id="status-detail-page">
			<Suspense fallback=|| view! { <LoadingBar /> }>
				{move || {
					detail_resource.get().map(|result| {
						match result {
							Ok(data) if data.server.kind == ServerKind::Central => {
								view! { <ServerDetailView data is_admin /> }.into_any()
							}
							Ok(data) => {
								view! { <ServerDetailView data is_admin /> }.into_any()
							}
							Err(err) => {
								view! { <div class="box has-danger-text">{err.to_string()}</div> }.into_any()
							}
						}
					})
				}}
			</Suspense>
		</div>
	}
}

#[component]
fn ServerDetailView(data: ServerDetailData, is_admin: Resource<bool>) -> impl IntoView {
	let data = Arc::new(data);

	view! {
		<div class="detail-container">
			<PageHeader data=data.clone() is_admin />
			<UrlSection data=data.clone() />
			<InfoSection status=data.last_status.clone() server=data.server.clone() />
			{(!data.child_servers.is_empty()).then(|| view! { <ChildServersSection data=data.clone() /> })}
			<aside class="legend">
				<VersionLegend />
				<StatusLegend />
			</aside>
		</div>
	}
}

#[component]
fn EditLink(server_id: Uuid, is_admin: Resource<bool>) -> impl IntoView {
	view! {
		<Transition>
			{move || {
				is_admin.get().unwrap_or(false).then(move || {
					view! {
						<A href=format!("/servers/{server_id}/edit") {..} class="button is-primary">"Edit"</A>
					}
				})
			}}
		</Transition>
	}
}

#[component]
fn PageHeader(data: Arc<ServerDetailData>, is_admin: Resource<bool>) -> impl IntoView {
	let server_id = data.server.id;

	view! {
		<div class="level">
			<div class="level-left">
				<div class="level-item">
					{let rank = data.server.rank; move || rank.map(|rank| view! { <ServerRankBadge rank /> })}
					<ServerKindBadge kind=data.server.kind />
				</div>
			</div>
			<h1 class="level-item is-size-3 status-dot-small">
				<StatusDot up=data.up name=data.server.name.clone().unwrap_or_default() />
				{data.child_servers.iter().map(|(up, child)| {
					view! {
						<StatusDot up=*up name=child.name.clone().unwrap_or_default() kind=child.kind />
					}
				}).collect_view().into_any()}
				{data.server.name.clone()}
			</h1>
			<div class="level-right">
				<div class="level-item">
					<EditLink server_id is_admin />
				</div>
			</div>
		</div>
	}
}

#[component]
fn UrlSection(data: Arc<ServerDetailData>) -> impl IntoView {
	view! {
		<div class="columns">
			<div class="column">
				<div class="box">
					<h2 class="is-size-5">URL</h2>
					<a class="header-url" href={data.server.host.clone()} target="_blank">{data.server.host.clone()}</a>
				</div>
			</div>
			{data.device_info.as_ref().map(|device| {
				view! {
					<div class="column">
						<div class="box">
							<h2 class="is-size-5">Device</h2>
							<DeviceShorty device=device.clone() />
						</div>
					</div>
				}
			})}
		</div>
	}
}

#[component]
fn InfoSection(
	server: Arc<ServerInfo>,
	status: Option<Arc<ServerLastStatusData>>,
) -> impl IntoView {
	view! {
		<section class="box">
			<div class="info-grid">
				{status.as_ref().map(|status| view! { <StatusInfo status=status.clone() /> })}
				{let listed = server.listed; (server.kind == ServerKind::Central).then(move || view! {
					<div class="info-item">
						<span class="info-label">"Mobile list"</span>
						<span class="info-value">{if listed { "Public" } else { "No" }}</span>
					</div>
				})}
				{server.parent_server_id.map({ let server = server.clone(); |id| {
					view! {
						<div class="info-item">
							<span class="info-label">"Parent"</span>
							<A href=format!("/servers/{id}") {..} class="info-value">
								{server.parent_server_name.clone().unwrap_or_else(|| id.to_string())}
							</A>
						</div>
					}
				}})}
				{server.cloud.map(|is_cloud| {
					view! {
						<div class="info-item">
							<span class="info-label">"Location"</span>
							{server.geolocation.map_or_else(|| view! {
								<span class="info-value">{ if is_cloud { "Cloud" } else { "On premise" }}</span>
							}.into_any(), |GeoPoint { lat, lon }| view! {
								<a href=format!("https://www.google.com/maps/search/?api=1&query={lat},{lon}") class="info-value" target="_blank">
									{if is_cloud {
										if let Some(region) = CloudRegion::from_lat_lon(lat, lon) {
											region.as_str()
										} else {
											"Cloud"
										}
									} else {
										"On premise"
									}}
								</a>
							}.into_any())}
						</div>
					}
				})}
			</div>

			{status.as_ref().map(|status| view! { <ExtraData status=status.clone() /> })}
		</section>
	}
}

// TODO: display a map with the locations of this and child servers

#[component]
fn StatusInfo(status: Arc<ServerLastStatusData>) -> impl IntoView {
	let min_chrome_version = status.min_chrome_version;
	view! {
		<div class:info-item>
			<span class="info-label">"Last seen"</span>
			<TimeAgo timestamp={status.created_at} {..} class:info-value />
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
	}
}

#[component]
fn ExtraData(status: Arc<ServerLastStatusData>) -> impl IntoView {
	let extra = status.extra.clone();
	if !extra.as_object().map(|o| o.is_empty()).unwrap_or(true) {
		view! {
			<details class="mt-5">
				<summary>"Extra Data"</summary>
				<pre>{serde_json::to_string_pretty(&extra).unwrap_or_default()}</pre>
			</details>
		}
		.into_any()
	} else {
		().into_any()
	}
}

#[component]
fn ChildServersSection(data: Arc<ServerDetailData>) -> impl IntoView {
	view! {
		<h2 class="is-size-4 mb-4">"Child servers (" {data.child_servers.len()} ")"</h2>
		{data.child_servers.iter().map(|(up, child)| {
			view! {
				<div class="box child-server">
					<StatusDot up=*up />
					<ServerShorty server=child.clone() />
				</div>
			}
		}).collect::<Vec<_>>()}
	}
}
