use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};

pub fn shell(options: LeptosOptions) -> impl IntoView {
	view! {
		<!DOCTYPE html>
		<html lang="en">
			<head>
				<meta charset="utf-8"/>
				<meta name="viewport" content="width=device-width, initial-scale=1"/>
				<Stylesheet id="main" href="/$/static/main.css" />
				<AutoReload options=options.clone() />
				<HydrationScripts options root="/$" />
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
	// Provides context that manages stylesheets, titles, meta tags, etc.
	provide_meta_context();

	// Creates a reactive value to update the button
	let count = RwSignal::new(0);
	let on_click = move |_| {
		leptos::logging::log!("Button clicked!");
		*count.write() += 1;
	};

	view! {
		<h1>"Welcome to Leptos!"</h1>
		<button on:click=on_click>"Click Me: " {count}</button>
	}
}
