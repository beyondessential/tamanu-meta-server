use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};

use crate::statuses::{summary, table};

pub fn shell(options: LeptosOptions) -> impl IntoView {
	view! {
		<!DOCTYPE html>
		<html lang="en">
			<head>
				<meta charset="utf-8"/>
				<meta name="viewport" content="width=device-width, initial-scale=1"/>
				<Stylesheet id="main" href="/$/static/main.css" />
				<AutoReload options=options.clone() />
				<HydrationScripts options islands=true root="/$" />
				<MetaTags/>
				<Title text="Tamanu Meta" />
			</head>
			<body>
				<App/>
			</body>
		</html>
	}
}

#[component]
pub fn Greeting() -> impl IntoView {
	let greeting = crate::statuses::greeting();

	view! {
		<Await future=greeting let:data>
			<div class="greeting">{data.clone().ok()}</div>
		</Await>
	}
}

#[island]
pub fn Status() -> impl IntoView {
	let (status_list_r, status_list_w) = signal(0);
	Effect::new(move |_| status_list_w.set(1));
	let data = Resource::new(move || status_list_r.get(), async |_| summary().await);

	view! {
		<Suspense fallback=|| view! { <div class="loading">"Loading…"</div> }>{move || {
			let data = data.get().and_then(|d| d.ok());
			view! {
				<p>
					{data.as_ref().map(|d| d.releases.len())} " release branches in active use: "
					<b>{data.as_ref().map(|d| d.releases.iter().map(|(maj, min)| format!("{}.{}", maj, min)).collect::<Vec<_>>().join(", "))}</b>
					<span class="versions">"("
					{data.as_ref().map(|d| d.versions.len())}
					" versions: "
					{data.as_ref().map(|d| d.bracket.min.to_string())}
					" — "
					{data.as_ref().map(|d| d.bracket.max.to_string())}
					")"</span>
				</p>
			}
		}}</Suspense>
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
								<td class="ago" title={entry.updated_at.clone()}>{entry.since.clone()} ago</td>
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

#[component]
pub fn App() -> impl IntoView {
	// Provides context that manages stylesheets, titles, meta tags, etc.
	provide_meta_context();

	view! {
		<header class="header">
			<Status/>
			<Greeting />
		</header>
		<Table />
	}
}
