use crate::fns::statuses::summary;
use leptos::prelude::*;

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
