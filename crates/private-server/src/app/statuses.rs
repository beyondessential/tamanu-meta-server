use leptos::prelude::*;
use leptos_meta::Stylesheet;

use crate::{
	app::status::Status,
	components::VersionIndicator,
	fns::statuses::{server_details, server_grouped_ids},
};

#[component]
pub fn Page() -> impl IntoView {
	view! {
		<Stylesheet id="css-status" href="/static/status.css" />
		<div id="status-page">
			<div class="page-header">
				<div class="header-info">
					<Status/>
				</div>
			</div>
			<ServerCards />
		</div>
	}
}

#[component]
pub fn ServerCards() -> impl IntoView {
	let (is_client, set_is_client) = signal(false);

	// Set to true only on client side
	Effect::new(move |_| {
		set_is_client.set(true);
	});

	let (trigger, set_trigger) = signal(0);
	let grouped_ids_resource =
		Resource::new(move || trigger.get(), async |_| server_grouped_ids().await);

	// Start loading only on client side
	Effect::new(move |_| {
		if is_client.get() {
			set_trigger.set(1);
		}
	});

	// Auto-reload every minute when page is visible
	#[cfg(not(feature = "ssr"))]
	Effect::new(move |_| {
		if is_client.get() {
			use wasm_bindgen::JsCast;
			use wasm_bindgen::closure::Closure;

			// Track last reload time
			let last_reload = std::rc::Rc::new(std::cell::Cell::new(web_sys::js_sys::Date::now()));

			// Set up interval for regular reloads
			let _ = leptos::prelude::set_interval(
				{
					let last_reload = last_reload.clone();
					move || {
						if let Some(document) = web_sys::window().and_then(|w| w.document()) {
							if !document.hidden() {
								last_reload.set(web_sys::js_sys::Date::now());
								set_trigger.update(|v| *v += 1);
							}
						}
					}
				},
				std::time::Duration::from_secs(60),
			);

			// Listen for visibility changes
			if let Some(document) = web_sys::window().and_then(|w| w.document()) {
				let last_reload_clone = last_reload.clone();
				let visibility_callback = Closure::wrap(Box::new(move || {
					if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
						if !doc.hidden() {
							let now = web_sys::js_sys::Date::now();
							let elapsed = now - last_reload_clone.get();

							// If more than 60 seconds since last reload, reload now
							if elapsed > 60_000.0 {
								last_reload_clone.set(now);
								set_trigger.update(|v| *v += 1);
							}
						}
					}
				}) as Box<dyn FnMut()>);

				let _ = document.add_event_listener_with_callback(
					"visibilitychange",
					visibility_callback.as_ref().unchecked_ref(),
				);

				// Keep the closure alive
				visibility_callback.forget();

				// Listen for custom reload event (can be triggered from console)
				// Usage: document.dispatchEvent(new Event('tamanu-reload-status'))
				let reload_event_callback = Closure::wrap(Box::new(move || {
					last_reload.set(web_sys::js_sys::Date::now());
					set_trigger.update(|v| *v += 1);
				}) as Box<dyn FnMut()>);

				let _ = document.add_event_listener_with_callback(
					"tamanu-reload-status",
					reload_event_callback.as_ref().unchecked_ref(),
				);

				// Keep the closure alive
				reload_event_callback.forget();
			}
		}
	});

	view! {
		<article>
			<Show when=move || is_client.get() fallback=|| view! {
				<div class="loading">"Loading servers..."</div>
			}>
				{move || {
					match grouped_ids_resource.get() {
						None => {
							// No data yet, show loading spinner
							view! { <div class="loading">"Loading…"</div> }.into_any()
						}
						Some(Ok(groups)) => {
							// Data loaded successfully, show the servers
							view! {
								<div class="grouped-servers">
									{groups.get("production").map(|ids| {
										view! { <RankSection rank="production" server_ids={ids.clone()} trigger={trigger} /> }.into_any()
									})}
									{groups.get("clone").map(|ids| {
										view! { <RankSection rank="clone" server_ids={ids.clone()} trigger={trigger} /> }.into_any()
									})}
									{groups.get("demo").map(|ids| {
										view! { <RankSection rank="demo" server_ids={ids.clone()} trigger={trigger} /> }.into_any()
									})}
									{groups.get("test").map(|ids| {
										view! { <RankSection rank="test" server_ids={ids.clone()} trigger={trigger} /> }.into_any()
									})}
									{groups.get("dev").map(|ids| {
										view! { <RankSection rank="dev" server_ids={ids.clone()} trigger={trigger} /> }.into_any()
									})}
								</div>
							}.into_any()
						}
						Some(Err(_)) => {
							// Error occurred, show nothing
							view! { <div></div> }.into_any()
						}
					}
				}}
			</Show>
		</article>
	}
}

#[component]
pub fn RankSection(
	rank: &'static str,
	server_ids: Vec<String>,
	trigger: ReadSignal<i32>,
) -> impl IntoView {
	if server_ids.is_empty() {
		return view! { <div></div> }.into_any();
	}

	let rank_display = match rank {
		"production" => "Production",
		"clone" => "Clone",
		"demo" => "Demo",
		"test" => "Test",
		"dev" => "Dev",
		_ => rank,
	};

	view! {
		<div class="rank-section">
			<h2 class="rank-heading">{rank_display}</h2>
			<div class="servers-grid">
				<For
					each=move || server_ids.clone()
					key=|id| id.clone()
					let:server_id
				>
					<ServerCardLoader server_id={server_id} trigger={trigger} />
				</For>
			</div>
		</div>
	}
	.into_any()
}

#[component]
pub fn ServerCardLoader(server_id: String, trigger: ReadSignal<i32>) -> impl IntoView {
	let server_id_clone = server_id.clone();
	let server_resource = Resource::new(
		move || (trigger.get(), server_id_clone.clone()),
		async |(_, id)| server_details(id).await,
	);

	view! {
		{move || {
			match server_resource.get() {
				None => {
					// No data yet, show loading spinner
					view! {
						<div class="server-card loading-card">
							<div class="loading-placeholder"></div>
						</div>
					}.into_any()
				}
				Some(Ok(server)) => {
					// Data loaded successfully, show the card
					view! { <ServerCard server={server} /> }.into_any()
				}
				Some(Err(err)) => {
					// Error occurred but we don't have old data, show nothing or error state
					view! { <div class="server-card">{err.to_string()}</div> }.into_any()
				}
			}
		}}
	}
}

#[component]
pub fn ServerCard(server: commons_types::server::cards::CentralServerCard) -> impl IntoView {
	let server_id = server.id.clone();
	let server_name = server.name.clone();
	let server_host = server.host.clone();
	let server_up = server.up.clone();
	let facility_servers = server.facility_servers.clone();
	let version = server.version.clone();
	let version_distance = server.version_distance;

	view! {
		<a href={format!("/servers/{}", server_id)} class="server-card">
			<a
				href={server_host.clone()}
				class="host-link"
				target="_blank"
				on:click=|e| e.stop_propagation()
				title={server_host.clone()}
			>
				"🌐"
			</a>
			<h3 class="server-name">{server_name.clone()}</h3>
			<div class="version-container">
				{version.map(|v| {
					view! {
						<VersionIndicator version={v} distance={version_distance} />
					}.into_any()
				})}
			</div>
			<div class="status-dots">
				<span
					class:status-dot class={server_up.to_string()}
					title={format!("{}: {}", server_name, server_up)}
				></span>
				<For
					each=move || facility_servers.clone()
					key=|facility| facility.id.clone()
					let:facility
				>
					<span
						class:status-dot class:facility-dot class={facility.up.to_string()}
						title={format!("{}: {}", facility.name, facility.up)}
					></span>
				</For>
			</div>
		</a>
	}
}
