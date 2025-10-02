use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::{
	components::{A, Route, Router, Routes},
	path,
};

mod admins;
mod greeting;
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
				<main>
					<Routes fallback=|| Index>
						<Route path=path!("") view=Index />
						<Route path=path!("status") view=statuses::Page />
						<Route path=path!("admins") view=admins::Page />
					</Routes>
				</main>
			</Router>
		</div>
	}
}

#[component]
pub fn Index() -> impl IntoView {
	view! {
		<Stylesheet id="index" href="/static/index.css" />
		<div id="index-page">
			<nav>
				<A href="/status">"Status"</A>
				<A href="/admins">"Admins"</A>
			</nav>
		</div>
	}
}
