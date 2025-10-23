use leptos::prelude::*;
use leptos::serde_json;
use leptos_meta::Stylesheet;
use leptos_router::hooks::use_params_map;

use crate::app::devices::DeviceListItem;
use crate::fns::statuses::server_detail;

#[component]
pub fn Page() -> impl IntoView {
	let params = use_params_map();
	let server_id = move || params.read().get("id").unwrap_or_default();

	let detail_resource =
		Resource::new(move || server_id(), async move |id| server_detail(id).await);

	view! {
		<Stylesheet id="status-detail" href="/static/deployment.css" />
		<div id="status-detail-page">
			<Suspense fallback=|| view! { <div class="loading">"Loading server details..."</div> }>
				{move || {
					detail_resource.get().map(|result| {
						match result {
							Ok(data) => {
								let server = data.server.clone();
								let device_info = data.device_info.clone();
								let last_status = data.last_status.clone();
								let up = data.up.clone();

								view! {
									<div class="detail-container">
										<div class="page-header">
											<div class="header-top">
												<a href="/status" class="back-link">"← Back to Status"</a>
											</div>
											<h1>
												<span class={format!("status-dot {}", up)} title={up.clone()}></span>
												{server.name.clone()}
											</h1>
											<span class="server-rank">{server.rank.clone()}</span>
										</div>

										<section class="detail-section">
											<h2>"Central server"</h2>
											<div class="info-grid">
												<div class="info-item">
													<span class="info-label">"ID"</span>
													<span class="info-value monospace">{server.id.clone()}</span>
												</div>
												<div class="info-item">
													<span class="info-label">"Host"</span>
													<span class="info-value">
														<a href={server.host.clone()} target="_blank">{server.host.clone()}</a>
													</span>
												</div>
												<div class="info-item">
													<span class="info-label">"Rank"</span>
													<span class="info-value">{server.rank.clone()}</span>
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

										{last_status.as_ref().map(|status| {
											let status = status.clone();
											view! {
												<section class="detail-section">
													<h2>"Latest status"</h2>
													<div class="info-grid">
														<div class="info-item">
															<span class="info-label">"Reported At"</span>
															<span class="info-value">{status.created_at.clone()}</span>
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
										})}
									</div>
								}.into_any()
							}
							Err(e) => {
								let error_msg = format!("Failed to load server details: {}", e);
								view! {
									<div class="error-container">
										<div class="page-header">
											<a href="/status" class="back-link">"← Back to Status"</a>
											<h1>"Error"</h1>
										</div>
										<div class="error-message">
											{error_msg}
										</div>
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
