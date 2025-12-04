use commons_types::server::rank::ServerRank;
use leptos::prelude::*;
use leptos_meta::Stylesheet;
use uuid::Uuid;

use crate::{
	components::{LoadingBar, ReleaseSummary, StatusLegend, VersionIndicator, VersionLegend},
	fns::statuses::{server_details, server_grouped_ids},
};

#[component]
pub fn Page() -> impl IntoView {
	view! {
		<Stylesheet id="css-status" href="/static/status.css" />
		<section class="section" id="status-page">
			<ReleaseSummary />
			<ServerCards />
			<aside class="legend">
				<VersionLegend />
				<StatusLegend />
			</aside>
		</section>
	}
}

#[component]
pub fn ServerCards() -> impl IntoView {
	#[cfg_attr(
		feature = "ssr",
		expect(unused_variables, reason = "set_trigger only used on the client")
	)]
	let (trigger, set_trigger) = signal(0);
	let grouped_ids_resource = LocalResource::new(async || server_grouped_ids().await);

	Effect::new({
		let resource = grouped_ids_resource;
		move |_| {
			trigger.get();
			resource.refetch();
		}
	});

	// Auto-reload every minute when page is visible
	#[cfg(not(feature = "ssr"))]
	Effect::new(move |_| {
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
	});

	view! {
		<article>
			<Transition fallback=|| view! { <LoadingBar /> }>
				{move || {
					grouped_ids_resource.get().map(|res| match res {
						Ok(groups) => {
							view! {
								{groups.into_iter().map(|(rank, ids)| {
									view! { <RankSection rank server_ids={ids.clone()} trigger /> }.into_any()
								}).collect::<Vec<_>>()}
							}.into_any()
						}
						Err(err) => {
							view! { {err} }.into_any()
						}
					})
				}}
			</Transition>
		</article>
	}
}

#[component]
pub fn RankSection(
	rank: ServerRank,
	server_ids: Vec<Uuid>,
	trigger: ReadSignal<i32>,
) -> impl IntoView {
	if server_ids.is_empty() {
		return view! { <div></div> }.into_any();
	}

	view! {
		<section>
			<h2>{rank}</h2>
			<div class="grid is-col-min-12 is-gap-2">
				<For
					each=move || server_ids.clone()
					key=|id| *id
					let:server_id
				>
					<ServerCardLoader server_id trigger {..} class="server-card cell box" />
				</For>
			</div>
		</section>
	}
	.into_any()
}

#[component]
pub fn ServerCardLoader(server_id: Uuid, trigger: ReadSignal<i32>) -> impl IntoView {
	let server_resource =
		LocalResource::new(move || async move { server_details(server_id).await });

	Effect::new(move || {
		trigger.get();
		server_resource.refetch();
	});

	view! {
		<a href={format!("/servers/{server_id}")}>
			<Transition fallback=move || view! { <div class="has-text-grey">"Thinking…"</div> }>
			{
				move || server_resource.get().map(|res| match res {
					Ok(server) => {
						view! { <ServerCard server /> }.into_any()
					}
					Err(err) => {
						view! { <div>{err}</div> }.into_any()
					}
				})
			}
			</Transition>
		</a>
	}
}

#[component]
pub fn ServerCard(server: commons_types::server::cards::CentralServerCard) -> impl IntoView {
	view! {
		<a
			href={server.host.clone()}
			class="host-link"
			target="_blank"
			on:click=|e| e.stop_propagation()
			title={server.host.clone()}
		>
			"🌐"
		</a>
		<h3 class="server-name">{server.name.clone()}</h3>
		<div class="version-container">
			{server.version.as_ref().map(|v| {
				view! {
					<VersionIndicator version={v.clone()} distance={server.version_distance} add_link=false />
				}
			})}
		</div>
		<div class="status-dots">
			<span
				class:status-dot class={server.up}
				title={format!("{}: {}", server.name.clone(), server.up)}
			></span>
			<For
				each={
					let facility_servers = server.facility_servers.clone();
					move || facility_servers.clone()
				}
				key=|facility| facility.id
				let:facility
			>
				<span
					class:status-dot class:facility-dot class={facility.up}
					title={format!("{}: {}", facility.name, facility.up)}
				></span>
			</For>
		</div>
	}
}
