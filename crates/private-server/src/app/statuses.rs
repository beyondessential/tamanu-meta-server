use leptos::prelude::*;
use leptos_meta::Stylesheet;

use crate::{
	app::{greeting::Greeting, status::Status},
	fns::statuses::table,
};

#[component]
pub fn Page() -> impl IntoView {
	view! {
		<Stylesheet id="status" href="/static/status.css" />
		<div id="status-page">
			<header class="header">
				<Status/>
				<Greeting />
			</header>
			<article>
				<Table />
			</article>
		</div>
	}
}

#[island]
pub fn Table() -> impl IntoView {
	let (entries_r, entries_w) = signal(0);
	Effect::new(move |_| entries_w.set(1));
	let data = Resource::new(move || entries_r.get(), async |_| table().await);

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
							each=move || data.get().and_then(|d| d.ok()).unwrap_or_default()
							key=|entry| entry.server_id.clone()
							let(entry)
						>
						<tr>
							<td
								class=format!("status {}", entry.up)
								on:click={
									let id = entry.server_id.clone();
									move |_| {
										web_sys::window().map(|window| {
											window.navigator().clipboard().write_text(&id)
										});
									}
								}
							>{entry.up.clone()}</td>
							<td class="name">{entry.server_name.clone()}</td>
							<td class="rank">{entry.server_rank.clone()}</td>
							<td class="host"><a href={entry.server_host.clone()}>{entry.server_host.clone()}</a></td>
							<Show
								when={ let up = entry.updated_at.is_some(); move || up }
								fallback=|| view! {
									<td class="ago never" title="never or more than a week ago">"<7d ago"</td>
									<td colspan=5></td>
								}
							>
								<td class="ago" title={entry.updated_at.clone()}>{entry.since.clone()} " ago"</td>
								<td class="version">{entry.version.clone()}</td>
								<td class="platform">{entry.platform.clone()}</td>
								<td class="nodejs">{entry.nodejs.clone()}</td>
								<td class="postgres">{entry.postgres.clone()}</td>
								<td class="timezone">{entry.timezone.clone()}</td>
							</Show>
						</tr>
						</For>
					</tbody>
				</table>
			}
		}}</Suspense>
	}
}
