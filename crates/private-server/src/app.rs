use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::{
	components::{A, ParentRoute, Redirect, Route, Router, Routes},
	path,
};

use crate::components::toast::Toast;

mod admins;
mod devices;
mod status;
mod statuses;

pub fn shell(options: LeptosOptions) -> impl IntoView {
	provide_meta_context();
	view! {
		<!DOCTYPE html>
		<html lang="en">
			<head>
				<meta charset="utf-8"/>
				<meta name="viewport" content="width=device-width, initial-scale=1"/>
				<Stylesheet id="main" href="/static/main.css" />
				<AutoReload options=options.clone() />
				<HydrationScripts options islands=true />
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
pub fn App() -> impl IntoView {
	view! {
		<div id="root">
			<Router>
				<GlobalNav />
				<Toast>
					<main>
						<Routes fallback=|| view! { <Redirect path="/status" /> }>
							<Route path=path!("") view=|| view! { <Redirect path="/status" /> } />
							<Route path=path!("status") view=statuses::Page />
							<Route path=path!("admins") view=admins::Page />
							 <ParentRoute path=path!("devices") view=devices::Page>
								<Route path=path!("") view=devices::Search />
								<Route path=path!("untrusted") view=devices::Untrusted />
								<Route path=path!("trusted") view=devices::Trusted />
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

	view! {
		<nav id="global-nav">
			<div class="nav-brand">
				<A href="/status">
					<img src="/static/images/tamanu_logo.svg" alt="Tamanu Logo" class="logo" />
				</A>
			</div>
			<div class="nav-links">
				<A href="/status">"Status"</A>
				<Suspense fallback=|| view! {}>
					{move || {
						is_admin.get().and_then(|result| {
							if result.unwrap_or(false) {
								Some(view! {
									<A href="/admins">"Admins"</A>
									<A href="/devices">"Devices"</A>
								})
							} else {
								None
							}
						})
					}}
				</Suspense>
				<Suspense fallback=|| view! {}>
					{move || {
						public_url.get().and_then(|result| {
							if let Ok(Some(url)) = result {
								Some(view! {
									<a href={url} target="_blank">"Public"</a>
								})
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
