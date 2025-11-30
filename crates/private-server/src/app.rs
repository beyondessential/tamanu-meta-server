use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::{
	components::{A, ParentRoute, Redirect, Route, Router, Routes},
	path,
};

use crate::components::Toast;

mod admins;
mod devices;
mod servers;
mod status;
mod statuses;
mod versions;

pub fn shell(options: LeptosOptions) -> impl IntoView {
	view! {
		<!DOCTYPE html>
		<html lang="en">
			<head>
				<meta charset="utf-8" />
				<meta name="viewport" content="width=device-width, initial-scale=1" />
				<AutoReload options=options.clone() />
				<HydrationScripts options />
				<MetaTags />
				<Title text="Tamanu Meta" />
			</head>
			<body>
				<App />
			</body>
			// There's a bug in leptos where the stylesheets are not being
			// replaced correctly when client-side navigation occurs.
			// Putting the main stylesheet at the bottom works around this
			// by ensuring that the dynamic stylesheets (from the page) are
			// swapped, but the main stylesheet is not.
			<Stylesheet id="css-main" href="/static/bulma/bulma.min.css" />
			<Stylesheet id="css-main" href="/static/main.css" />
		</html>
	}
}

#[component]
pub fn App() -> impl IntoView {
	provide_meta_context();
	view! {
		<div id="root">
			<Router>
				<GlobalNav />
				<Toast>
					<main>
						<Routes fallback=|| view! { <Redirect path="/status" /> }>
							<Route path=path!("") view=|| view! { <Redirect path="/status" /> } />
							<Route path=path!("status") view=statuses::Page />
							<ParentRoute path=path!("servers") view=servers::Page>
								<Route path=path!("") view=servers::Centrals />
								<Route path=path!("facilities") view=servers::Facilities />
								<Route path=path!(":id/edit") view=servers::Edit />
								<Route path=path!(":id") view=servers::Detail />
							</ParentRoute>
							<Route path=path!("admins") view=admins::Page />
							<Route path=path!("versions") view=versions::Page />
							<Route path=path!("versions/:version") view=versions::Detail />

							<ParentRoute path=path!("devices") view=devices::Page>
								<Route path=path!("") view=devices::Search />
								<Route path=path!("untrusted") view=devices::Untrusted />
								<Route path=path!("trusted") view=devices::Trusted />
								<Route path=path!(":id") view=devices::Detail />
							</ParentRoute>
						</Routes>
					</main>
				</Toast>
			</Router>
		</div>
	}
}

#[component]
pub fn GlobalNav() -> impl IntoView {
	let is_admin = Resource::new(
		|| (),
		|_| async { crate::fns::commons::is_current_user_admin().await },
	);

	let public_url = Resource::new(|| (), |_| async { crate::fns::commons::public_url().await });

	let server_versions_url = Resource::new(
		|| (),
		|_| async { crate::fns::commons::server_versions_url().await },
	);

	view! {
		<nav id="global-nav">
			<div class="nav-brand">
				<A href="/status">
					<img src="/static/images/tamanu_logo.svg" alt="Tamanu Logo" class="logo" />
				</A>
			</div>
			<div class="nav-links">
				<A href="/status">"Status"</A>
				<A href="/servers">"Servers"</A>
				<A href="/versions">"Versions"</A>
				<Suspense>
					{move || {
						is_admin
							.get()
							.and_then(|result| {
								if result.unwrap_or(false) {
									Some(
										view! {
											<A href="/admins">"Admins"</A>
											<A href="/devices">"Devices"</A>
										},
									)
								} else {
									None
								}
							})
					}}
				</Suspense>
				<Suspense>
					{move || {
						public_url
							.get()
							.and_then(|result| {
								if let Ok(Some(url)) = result {
									Some(
										view! {
											<a href=url target="_blank">
												"Public"
											</a>
										},
									)
								} else {
									None
								}
							})
					}}
				</Suspense>
				<Suspense>
					{move || {
						server_versions_url
							.get()
							.and_then(|result| {
								if let Ok(Some(url)) = result {
									Some(
										view! {
											<a
												href=url
												target="_blank"
												style="font-size: 0.7em; text-align: center; padding: 0.25em 1em;"
											>
												"Server"
												<br />
												"Versions"
											</a>
										},
									)
								} else {
									None
								}
							})
					}}
				</Suspense>
			</div>
		</nav>
	}
}
