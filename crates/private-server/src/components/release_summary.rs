use itertools::intersperse_with;
use leptos::prelude::*;

use crate::fns::statuses::summary;

#[component]
pub fn ReleaseSummary() -> impl IntoView {
	let (status_list_r, status_list_w) = signal(0);
	Effect::new(move |_| status_list_w.set(1));
	let data = Resource::new(move || status_list_r.get(), async |_| summary().await);

	view! {
		<aside class="box release-summary">
			<Suspense fallback=|| view! { <div class="loading">"Loading…"</div> }>{move || {
				let data = data.get().and_then(|d| d.ok());
				view! {
					<div>
					{data.as_ref().map(|d| d.releases.len())} " release branches in active use: "
					{data.as_ref().map(|d| intersperse_with(
						d.releases.iter().map(|(maj, min)| view! { <b class="version">{*maj} "." {*min}</b> }.into_any()),
						|| view! { ", " }.into_any()
					).collect::<Vec<_>>())}
					" ("
					{data.as_ref().map(|d| d.versions.len())}
					" versions: "
					<span class="version">{data.as_ref().map(|d| d.bracket.min.to_string())}</span>
					" — "
					<span class="version">{data.as_ref().map(|d| d.bracket.max.to_string())}</span>
					")"
					</div>
				}
			}}</Suspense>
		</aside>
	}
}
