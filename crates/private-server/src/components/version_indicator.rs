use commons_types::version::VersionStr;
use leptos::prelude::*;

#[component]
pub fn VersionIndicator(
	/// The version string to display (e.g., "2.10.0")
	version: VersionStr,
	/// Optional distance from the latest published version
	#[prop(default = None)]
	distance: Option<u64>,
) -> impl IntoView {
	let Some(distance) = distance else {
		return view! {
			<span class="version-display">
				<span class="version-text">{version.to_string()}</span>
				<span class:version-indicator class="version-unknown" title="Unknown version"></span>
			</span>
		}
		.into_any();
	};

	let title = format!("{} versions behind latest", distance);
	let color_class = match distance {
		d if d < 2 => "version-up-to-date",
		d if d >= 10 => "version-very-outdated",
		d if d >= 5 => "version-outdated",
		_ => "version-okay",
	};

	view! {
		<span class="version-display">
			<span class="version-text">{version.to_string()}</span>
			<span class:version-indicator class={color_class} title={title}></span>
		</span>
	}
	.into_any()
}
