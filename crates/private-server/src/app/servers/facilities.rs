use leptos::prelude::*;
use leptos_meta::Stylesheet;

use crate::fns::servers::{ServerListItem, list_facility_servers};

#[component]
pub fn Facilities() -> impl IntoView {
	let servers = Resource::new(|| (), |_| async { list_facility_servers().await });

	view! {
		<Stylesheet id="css-servers" href="/static/servers.css" />
		<div id="servers-list-page">
			<Suspense fallback=|| view! { <div class="loading">"Loading facility servers..."</div> }>
				{move || {
					servers.get().map(|result| {
						match result {
							Ok(server_list) => {
								if server_list.is_empty() {
									view! {
										<div class="empty-state">
											<p>"No facility servers found."</p>
										</div>
									}.into_any()
								} else {
									// Split into unaffiliated, unnamed, and named sections
									let (unaffiliated, affiliated): (Vec<_>, Vec<_>) = server_list
										.into_iter()
										.partition(|s| s.parent_server_id.is_none());

									let (unaffiliated_unnamed, unaffiliated_named): (Vec<_>, Vec<_>) = unaffiliated
										.into_iter()
										.partition(|s| s.name.is_none());

									let (affiliated_unnamed, affiliated_named): (Vec<_>, Vec<_>) = affiliated
										.into_iter()
										.partition(|s| s.name.is_none());

									let has_special_section = !unaffiliated_unnamed.is_empty() || !unaffiliated_named.is_empty() || !affiliated_unnamed.is_empty();
									let has_regular_section = !affiliated_named.is_empty();

									view! {
										<div class="servers-grid">
											{unaffiliated_unnamed.into_iter().map(|server| {
												view! {
													<ServerCard server=server />
												}
											}).collect::<Vec<_>>()}
											{unaffiliated_named.into_iter().map(|server| {
												view! {
													<ServerCard server=server />
												}
											}).collect::<Vec<_>>()}
											{affiliated_unnamed.into_iter().map(|server| {
												view! {
													<ServerCard server=server />
												}
											}).collect::<Vec<_>>()}
											{if has_special_section && has_regular_section {
												view! { <div class="section-break"></div> }.into_any()
											} else {
												().into_any()
											}}
											{affiliated_named.into_iter().map(|server| {
												view! {
													<ServerCard server=server />
												}
											}).collect::<Vec<_>>()}
										</div>
									}.into_any()
								}
							}
							Err(e) => {
								view! {
									<div class="error-message">
										{format!("Failed to load facility servers: {}", e)}
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

	let server_id = server.id.clone();
	let is_admin = Resource::new(
		|| (),
		|_| async { crate::fns::commons::is_current_user_admin().await },
	);

	view! {
		<div class={card_class}>
			<a href={format!("/servers/{}", server.id)} class="server-card-link">
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
					{server.parent_server_name.as_ref().map(|parent_name| {
						let parent_name = parent_name.clone();
						view! {
							<div class="server-info">
								<span class="label">"Parent:"</span>
								<span class="value">{parent_name}</span>
							</div>
						}
					})}
				</div>
			</a>
			<Suspense>
				{move || {
					is_admin.get().and_then(|result| {
						if result.ok().unwrap_or(false) {
							Some(view! {
								<a href={format!("/servers/{}/edit", server_id)} class="edit-button-link">
									<button class="edit-button">"Edit"</button>
								</a>
							})
						} else {
							None
						}
					})
				}}
			</Suspense>
		</div>
	}
}
