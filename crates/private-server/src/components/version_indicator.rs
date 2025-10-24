use leptos::prelude::*;

#[component]
pub fn VersionIndicator(
	/// The version string to display (e.g., "2.10.0")
	version: String,
	/// Optional distance from the latest published version
	#[prop(default = None)]
	distance: Option<i32>,
) -> impl IntoView {
	let title = format!("{} versions behind latest", distance.unwrap_or(-1));
	let color_class = match distance {
		Some(d) if d < 2 => "version-up-to-date",
		Some(d) if d >= 10 => "version-very-outdated",
		Some(d) if d >= 5 => "version-outdated",
		Some(_) => "version-okay",
		None => "version-unknown",
	};

	view! {
		<span class="version-display">
			<span class="version-text">{version}</span>
			<span class:version-indicator class={color_class} title={title}></span>
		</span>
	}
}
