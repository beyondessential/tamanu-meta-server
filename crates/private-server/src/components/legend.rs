use commons_types::status::ShortStatus;
use leptos::prelude::*;

use crate::components::{StatusDot, version_indicator::VersionSquare};

#[component]
pub fn VersionLegend() -> impl IntoView {
	view! {
		<p>
			<span class="legend-item">
				<VersionSquare distance=Some(1) />
				<span class="legend-label">"Up to date"</span>
			</span>
			" "
			<span class="legend-item">
				<VersionSquare distance=Some(3) />
				<span class="legend-label">"2-4 versions behind"</span>
			</span>
			" "
			<span class="legend-item">
				<VersionSquare distance=Some(7) />
				<span class="legend-label">"5-9 versions behind"</span>
			</span>
			" "
			<span class="legend-item">
				<VersionSquare distance=Some(11) />
				<span class="legend-label">"10+ versions behind"</span>
			</span>
			" "
			<span class="legend-item">
				<VersionSquare distance=None />
				<span class="legend-label">"Version not known"</span>
			</span>
		</p>
	}
}

#[component]
pub fn StatusLegend() -> impl IntoView {
	view! {
		<p>
			<span class="legend-item">
				<StatusDot up=ShortStatus::Up />
				<span class="legend-label">"Up (seen a minute ago)"</span>
			</span>
			" "
			<span class="legend-item">
				<StatusDot up=ShortStatus::Blip />
				<span class="legend-label">"Blip (missed 2 checks)"</span>
			</span>
			" "
			<span class="legend-item">
				<StatusDot up=ShortStatus::Away />
				<span class="legend-label">"Away (last seen 2-10m ago)"</span>
			</span>
			" "
			<span class="legend-item">
				<StatusDot up=ShortStatus::Down />
				<span class="legend-label">"Down (last seen 10m-7d ago)"</span>
			</span>
			" "
			<span class="legend-item">
				<StatusDot up=ShortStatus::Gone />
				<span class="legend-label">"Gone (never or more than 7d ago)"</span>
			</span>
		</p>
	}
}
