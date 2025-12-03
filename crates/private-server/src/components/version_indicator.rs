use commons_types::version::VersionStr;
use leptos::prelude::*;

#[component]
pub fn VersionSquare(distance: Option<u64>) -> impl IntoView {
	view! {
		<span
			class:version-indicator
			class={match distance {
				Some(d) if d < 2 => "version-up-to-date",
				Some(d) if d >= 10 => "version-very-outdated",
				Some(d) if d >= 5 => "version-outdated",
				Some(_) => "version-okay",
				None => "version-unknown",
			}}
			title=distance.map_or("Unknown version".into(), |d| format!("{d} versions behind latest"))
		></span>
	}
}

#[component]
pub fn VersionIndicator(
	/// The version string to display (e.g., "2.10.0")
	version: VersionStr,
	/// Optional distance from the latest published version
	#[prop(default = None)]
	distance: Option<u64>,
	/// Add a link to the version page
	#[prop(default = true)]
	add_link: bool,
) -> impl IntoView {
	let version_str = version.to_string();
	let version_link = format!("/versions/{}", version_str);

	if add_link {
		view! {
			<a href={version_link} class="version-display version-link">
				<span class="version-text">{version_str}</span>
				<VersionSquare distance />
			</a>
		}
		.into_any()
	} else {
		view! {
			<span class="version-display">
				<span class="version-text">{version_str}</span>
				<VersionSquare distance />
			</span>
		}
		.into_any()
	}
}
