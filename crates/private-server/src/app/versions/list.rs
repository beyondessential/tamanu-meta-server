use commons_types::version::VersionStatus;
use leptos::prelude::*;
use leptos_meta::Stylesheet;
use leptos_router::components::A;

use crate::{
	components::{ErrorHandler, LoadingBar, Nothing, VersionStatusBadge},
	fns::versions::{MinorVersionGroup, get_grouped_versions},
};

#[component]
pub fn Page() -> impl IntoView {
	let grouped_versions = Resource::new(|| (), |_| async { get_grouped_versions().await });

	view! {
		<Stylesheet id="css-versions" href="/static/versions.css" />
		<h1 class="is-size-3 my-4">"Versions"</h1>
		<Transition fallback=|| view! { <LoadingBar /> }>
			<ErrorHandler>
				{move || grouped_versions.and_then(|groups| {
					let groups = groups.clone();
					if groups.is_empty() {
						view! { <Nothing thing="versions" /> }.into_any()
					} else {
						view! {
							<For each=move || groups.clone() key=|g| (g.major, g.minor) let:group>
								<MinorVersionGroupComponent group={group} />
							</For>
						}.into_any()
					}
				})}
			</ErrorHandler>
		</Transition>
	}
}

#[component]
pub fn MinorVersionGroupComponent(group: MinorVersionGroup) -> impl IntoView {
	let major = group.major;
	let minor = group.minor;
	let count = group.count;
	let latest_patch = group.latest_patch;
	let first_created_at = group.first_created_at;
	let versions = group.versions.clone();

	view! {
		<details class="box minor-version-group monospace">
			<summary class="level">
				<div class="level-left">
					<div class="group-version">
						{major} "." {minor}
						<span class="version-patch">"." {latest_patch}</span>
					</div>
				</div>
				<div class="level-right">
					<div class="group-details">
						<p>{format!("{} version{}", count, if count == 1 { "" } else { "s" })}</p>
						<p>{first_created_at.strftime("%Y-%m-%d").to_string()}</p>
					</div>
				</div>
			</summary>
			<div class="minor-versions">
				<For each=move || versions.clone() key=|v| (v.major, v.minor, v.patch) let:v>
					<A
						href={format!("/versions/{}.{}.{}", v.major, v.minor, v.patch)}
						{..}
						class="level box minor-version"
						class:has-background-warning-light={v.status == VersionStatus::Draft}
						class:has-background-danger-light={v.status == VersionStatus::Yanked}
					>
						<div class="level-left">
							<div class="level-item grouped-version">
								{v.major} "." {v.minor}
								<span class="version-patch">"." {v.patch}</span>
							</div>
							{(v.status != VersionStatus::Published).then(|| {
								view! {
									<VersionStatusBadge status={v.status} />
								}
							})}
						</div>
						<div class="level-right">
							<div class="level-item">{v.created_at.strftime("%Y-%m-%d").to_string()}</div>
						</div>
					</A>
				</For>
			</div>
		</details>
	}
}
