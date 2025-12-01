use leptos::prelude::*;

#[component]
pub fn VersionLegend() -> impl IntoView {
	view! {
		<p>
			<span class="legend-item">
				<span class="version-indicator version-up-to-date"></span>
				" "
				<span class="legend-label">"Up to date"</span>
			</span>
			" "
			<span class="legend-item">
				<span class="version-indicator version-okay"></span>
				" "
				<span class="legend-label">"2-4 versions behind"</span>
			</span>
			" "
			<span class="legend-item">
				<span class="version-indicator version-outdated"></span>
				" "
				<span class="legend-label">"5-9 versions behind"</span>
			</span>
			" "
			<span class="legend-item">
				<span class="version-indicator version-very-outdated"></span>
				" "
				<span class="legend-label">"10+ versions behind"</span>
			</span>
		</p>
	}
}

#[component]
pub fn StatusLegend() -> impl IntoView {
	view! {
		<p>
			<span class="legend-item">
				<span class="status-dot up"></span>
				" "
				<span class="legend-label">"Up (seen a minute ago)"</span>
			</span>
			" "
			<span class="legend-item">
				<span class="status-dot blip"></span>
				" "
				<span class="legend-label">"Blip (missed 2 checks)"</span>
			</span>
			" "
			<span class="legend-item">
				<span class="status-dot away"></span>
				" "
				<span class="legend-label">"Away (last seen 2-10m ago)"</span>
			</span>
			" "
			<span class="legend-item">
				<span class="status-dot down"></span>
				" "
				<span class="legend-label">"Down (last seen 10m-7d ago)"</span>
			</span>
			" "
			<span class="legend-item">
				<span class="status-dot gone"></span>
				" "
				<span class="legend-label">"Gone (never or more than 7d ago)"</span>
			</span>
		</p>
	}
}
