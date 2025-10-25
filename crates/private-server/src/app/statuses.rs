use commons_types::server::cards::CentralServerCard;
use leptos::prelude::*;
use leptos_meta::Stylesheet;

use crate::{
	app::status::Status, components::VersionIndicator, fns::statuses::grouped_central_servers,
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
	let grouped_servers_resource = Resource::new(
		move || trigger.get(),
		async |_| grouped_central_servers().await,
	);

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
				<Suspense fallback=|| view! { <div class="loading">"Loading…"</div> }>{move || {
					grouped_servers_resource.get().and_then(|result| result.ok()).map(|data| {
						view! {
							<div class="grouped-servers">
								<RankSection rank="production" servers={data.production.clone()} />
								<RankSection rank="clone" servers={data.clone.clone()} />
								<RankSection rank="demo" servers={data.demo.clone()} />
								<RankSection rank="test" servers={data.test.clone()} />
								<RankSection rank="dev" servers={data.dev.clone()} />
							</div>
						}
					})
				}}</Suspense>
			</Show>
		</article>
	}
}

#[component]
pub fn RankSection(rank: &'static str, servers: Vec<CentralServerCard>) -> impl IntoView {
	if servers.is_empty() {
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
					each=move || servers.clone()
					key=|server| server.id.clone()
					let:server
				>
					<ServerCard server={server} />
				</For>
			</div>
		</div>
	}
	.into_any()
}

#[component]
pub fn ServerCard(server: CentralServerCard) -> impl IntoView {
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
