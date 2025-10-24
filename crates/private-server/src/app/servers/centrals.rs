use leptos::prelude::*;
use leptos_meta::Stylesheet;

use crate::fns::servers::{ServerListItem, list_central_servers};

#[component]
pub fn Centrals() -> impl IntoView {
	let servers = Resource::new(|| (), |_| async { list_central_servers().await });

	view! {
		<Stylesheet id="css-servers" href="/static/servers.css" />
		<div id="servers-list-page">
			<Suspense fallback=|| view! { <div class="loading">"Loading central servers..."</div> }>
				{move || {
					servers.get().map(|result| {
						match result {
							Ok(server_list) => {
								if server_list.is_empty() {
									view! {
										<div class="empty-state">
											<p>"No central servers found."</p>
										</div>
									}.into_any()
								} else {
									// Split into unnamed and named sections
									let (unnamed, named): (Vec<_>, Vec<_>) = server_list
										.into_iter()
										.partition(|s| s.name.is_none());

									let has_unnamed = !unnamed.is_empty();
									let has_named = !named.is_empty();

									view! {
										<div class="servers-grid">
											{unnamed.into_iter().map(|server| {
												view! {
													<ServerCard server=server />
												}
											}).collect::<Vec<_>>()}
											{if has_named && has_unnamed {
												view! { <div class="section-break"></div> }.into_any()
											} else {
												().into_any()
											}}
											{named.into_iter().map(|server| {
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
										{format!("Failed to load central servers: {}", e)}
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
