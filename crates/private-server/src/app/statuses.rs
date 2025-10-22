use leptos::prelude::*;
use leptos_meta::Stylesheet;

use crate::{
	app::status::Status,
	fns::statuses::{server_details, server_ids, server_status},
};

#[derive(Clone, Copy)]
struct ReloadContext {
	trigger: ReadSignal<i32>,
	set_trigger: WriteSignal<i32>,
}

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
			<Table />
		</div>
	}
}

#[component]
pub fn Table() -> impl IntoView {
	view! {
		<article>
			<TableIsland />
		</article>
	}
}

#[island]
pub fn TableIsland() -> impl IntoView {
	let (is_client, set_is_client) = signal(false);

	// Set to true only on client side
	Effect::new(move |_| {
		set_is_client.set(true);
	});

	let (trigger, set_trigger) = signal(0);
	let server_ids_resource = Resource::new(move || trigger.get(), async |_| server_ids().await);

	let (reload_trigger, set_reload_trigger) = signal(0);
	let reload_ctx = ReloadContext {
		trigger: reload_trigger,
		set_trigger: set_reload_trigger,
	};
	provide_context(reload_ctx);

	// Start loading only on client side
	Effect::new(move |_| {
		if is_client.get() {
			set_trigger.set(1);
		}
	});

	view! {
		<div>
			<Show when=move || is_client.get() fallback=|| view! {
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
						<tr><td colspan=10 class="loading">"Loading servers..."</td></tr>
					</tbody>
				</table>
			}>
				<button on:click=move |_| reload_ctx.set_trigger.update(|n| *n += 1)>
					"Reload"
				</button>
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
			</Show>
		</div>
	}
}

#[component]
pub fn ServerRow(server_id: String) -> impl IntoView {
	leptos::logging::log!("ServerRow created for {}", server_id);

	let reload_ctx = expect_context::<ReloadContext>();

	let server_id_for_details = server_id.clone();
	let details = Resource::new(
		move || {
			let trigger = reload_ctx.trigger.get();
			leptos::logging::log!(
				"Details source function called for {} with trigger {}",
				server_id_for_details,
				trigger
			);
			(server_id_for_details.clone(), trigger)
		},
		|(id, _)| async move { server_details(id).await },
	);

	let server_id_for_status = server_id.clone();
	let status = Resource::new(
		move || {
			let trigger = reload_ctx.trigger.get();
			leptos::logging::log!(
				"Status source function called for {} with trigger {}",
				server_id_for_status,
				trigger
			);
			(server_id_for_status.clone(), trigger)
		},
		|(id, _)| async move { server_status(id).await },
	);

	view! {
		<Transition fallback=|| view! { <tr><td colspan=10>"Loading…"</td></tr> }>
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
								{
									if status.updated_at.is_some() {
										view! {
											<td class="ago" title={status.updated_at.clone()}>{status.since.clone()} " ago"</td>
											<td class="version monospace">{status.version.clone()}</td>
											<td class="platform monospace">{status.platform.clone()}</td>
											<td class="nodejs monospace">{status.nodejs.clone()}</td>
											<td class="postgres monospace">{status.postgres.clone()}</td>
											<td class="timezone">{status.timezone.clone()}</td>
										}.into_any()
									} else {
										view! {
											<td class="ago never" title="never or more than a week ago">"<7d ago"</td>
											<td class="version monospace"></td>
											<td class="platform monospace"></td>
											<td class="nodejs monospace"></td>
											<td class="postgres monospace"></td>
											<td class="timezone"></td>
										}.into_any()
									}
								}
							</tr>
						}.into_any()
					}
					_ => view! { <tr><td colspan=10>"Loading…"</td></tr> }.into_any(),
				}
			}}
		</Transition>
	}
}
