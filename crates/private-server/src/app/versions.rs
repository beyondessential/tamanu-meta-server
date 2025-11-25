use leptos::prelude::*;
use leptos_meta::Stylesheet;

use crate::fns::versions::{MinorVersionGroup, get_grouped_versions};

#[component]
pub fn Page() -> impl IntoView {
	view! {
		<Stylesheet id="css-versions" href="/static/versions.css" />
		<div id="versions-page">
			<div class="page-header">
				<h1>"Versions"</h1>
			</div>
			<VersionsList />
		</div>
	}
}

#[component]
pub fn VersionsList() -> impl IntoView {
	let grouped_versions = Resource::new(|| (), |_| async { get_grouped_versions().await });

	view! {
		<div class="versions-list">
			<Suspense fallback=|| view! { <div class="loading">"Loading versions..."</div> }>
				{move || grouped_versions.get().map(|data| match data {
					Ok(groups) => {
						if groups.is_empty() {
							view! {
								<div class="no-versions">"No versions found"</div>
							}.into_any()
						} else {
							view! {
								<div class="version-groups">
									<For each=move || groups.clone() key=|g| (g.major, g.minor) let:group>
										<MinorVersionGroupComponent group={group} />
									</For>
								</div>
							}.into_any()
						}
					}
					Err(e) => {
						view! {
							<div class="error">{format!("Error loading versions: {}", e)}</div>
						}.into_any()
					}
				})}
			</Suspense>
		</div>
	}
}

#[component]
pub fn MinorVersionGroupComponent(group: MinorVersionGroup) -> impl IntoView {
	let (expanded, set_expanded) = signal(false);

	let toggle_expanded = move |_| {
		set_expanded.update(|e| *e = !*e);
	};

	let major = group.major;
	let minor = group.minor;
	let count = group.count;
	let latest_patch = group.latest_patch;
	let first_created_at = group.first_created_at.clone();
	let versions = group.versions.clone();

	view! {
		<div class="version-group">
			<div class="group-header" on:click=toggle_expanded>
				<div class="version-number">
					{major} "." {minor}
					<span class="version-patch">"." {latest_patch}</span>
				</div>
				<div class="details">
					<div>{format!("{} version{}", count, if count == 1 { "" } else { "s" })}</div>
					<div>{first_created_at.clone()}</div>
				</div>
			</div>
			<div class="group-content" class:expanded=move || expanded.get()>
				<div class="versions">
					<For each=move || versions.clone() key=|v| (v.major, v.minor, v.patch) let:v>
						<div class:version-item class={format!("version-status-{}", v.status.to_lowercase())}>
							<div class="version-number">
								{v.major} "." {v.minor}
								<span class="version-patch">"." {v.patch}</span>
							</div>
							{(v.status.to_lowercase() != "published").then(|| {
								view! {
									<div class="version-status">{v.status.clone()}</div>
								}
							})}
							<div class="version-date">{v.created_at.clone()}</div>
						</div>
					</For>
				</div>
			</div>
		</div>
	}
}
