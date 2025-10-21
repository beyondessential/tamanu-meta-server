use leptos::prelude::*;
use leptos_meta::Stylesheet;

use crate::{
	app::status::Status,
	fns::statuses::{server_details, server_ids, server_status},
};

#[component]
pub fn Page() -> impl IntoView {
	view! {
		<Stylesheet id="status" href="/static/status.css" />
		<div id="status-page">
			<div class="page-header">
				<div class="header-info">
					<Status/>
				</div>
			</div>
			<article>
				<Table />
			</article>
		</div>
	}
}

#[island]
pub fn Table() -> impl IntoView {
	let (trigger, set_trigger) = signal(0);
	Effect::new(move |_| set_trigger.set(1));
	let server_ids_resource = Resource::new(move || trigger.get(), async |_| server_ids().await);

	view! {
		<Suspense fallback=|| view! { <div class="loading">"Loading…"</div> }>{move || {
			view! {
				<table>
					<thead>
					  <tr>
						<th class="status">Status</th>
						<th class="name">Name</th>
						<th class="rank">Rank</th>
						<th class="host">Host</th>
						<th class="ago">Last seen</th>
						<th class="version">Version</th>
						<th class="platform">Platform</th>
						<th class="nodejs">Node.js</th>
						<th class="postgres">Postgres</th>
						<th class="timezone">Timezone</th>
					  </tr>
					</thead>
					<tbody>
						<For
							each=move || server_ids_resource.get().and_then(|d| d.ok()).unwrap_or_default()
							key=|id| id.clone()
							let:server_id
						>
							<ServerRow server_id={server_id.clone()} />
						</For>
					</tbody>
				</table>
			}
		}}</Suspense>
	}
}

#[component]
pub fn ServerRow(server_id: String) -> impl IntoView {
	let server_id_for_details = server_id.clone();
	let details = Resource::new(
		move || server_id_for_details.clone(),
		|id| async move { server_details(id).await },
	);

	let server_id_for_status = server_id.clone();
	let status = Resource::new(
		move || server_id_for_status.clone(),
		|id| async move { server_status(id).await },
	);

	view! {
		<Suspense fallback=|| view! { <tr><td colspan=10>"Loading…"</td></tr> }>
			{move || {
				let details_data = details.get().and_then(|d| d.ok());
				let status_data = status.get().and_then(|s| s.ok());

				match (details_data, status_data) {
					(Some(details), Some(status)) => {
						view! {
							<tr>
								<td
									class=format!("status {}", status.up)
									on:click={
										let id = details.id.clone();
										move |_| {
											web_sys::window().map(|window| {
												window.navigator().clipboard().write_text(&id)
											});
										}
									}
								>{status.up.clone()}</td>
								<td class="name">{details.name.clone()}</td>
								<td class="rank">{details.rank.clone()}</td>
								<td class="host"><a href={details.host.clone()}>{details.host.clone()}</a></td>
								<Show
									when={ let up = status.updated_at.is_some(); move || up }
									fallback=|| view! {
										<td class="ago never" title="never or more than a week ago">"<7d ago"</td>
										<td colspan=5></td>
									}
								>
									<td class="ago" title={status.updated_at.clone()}>{status.since.clone()} " ago"</td>
									<td class="version monospace">{status.version.clone()}</td>
									<td class="platform monospace">{status.platform.clone()}</td>
									<td class="nodejs monospace">{status.nodejs.clone()}</td>
									<td class="postgres monospace">{status.postgres.clone()}</td>
									<td class="timezone">{status.timezone.clone()}</td>
								</Show>
							</tr>
						}.into_any()
					}
					_ => view! { <tr><td colspan=10>"Error loading server data"</td></tr> }.into_any(),
				}
			}}
		</Suspense>
	}
}
