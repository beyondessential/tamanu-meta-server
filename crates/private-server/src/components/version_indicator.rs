use commons_types::version::VersionStr;
use leptos::prelude::*;

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

	let Some(distance) = distance else {
		return if add_link {
			view! {
				<a href={version_link.clone()} class="version-display version-link">
					<span class="version-text">{version_str}</span>
					<span class:version-indicator class="version-unknown" title="Unknown version"></span>
				</a>
			}
			.into_any()
		} else {
			view! {
				<span class="version-display">
					<span class="version-text">{version_str}</span>
					<span class:version-indicator class="version-unknown" title="Unknown version"></span>
				</span>
			}
			.into_any()
		};
	};

	let title = format!("{} versions behind latest", distance);
	let color_class = match distance {
		d if d < 2 => "version-up-to-date",
		d if d >= 10 => "version-very-outdated",
		d if d >= 5 => "version-outdated",
		_ => "version-okay",
	};

	if add_link {
		view! {
			<a href={version_link} class="version-display version-link">
				<span class="version-text">{version_str}</span>
				<span class:version-indicator class={color_class} title={title}></span>
			</a>
		}
		.into_any()
	} else {
		view! {
			<span class="version-display">
			<span class="version-text">{version_str}</span>
			<span class:version-indicator class={color_class} title={title}></span>
			</span>
		}
		.into_any()
	}
}
