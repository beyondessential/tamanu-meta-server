use leptos::prelude::*;
use leptos_meta::Stylesheet;
use leptos_router::hooks::use_params_map;

use crate::fns::versions::{
	ArtifactData, VersionDetail, get_version_artifacts, get_version_detail,
	update_version_changelog, update_version_status,
};
use commons_types::version::VersionStatus;

#[component]
pub fn Detail() -> impl IntoView {
	view! {
		<Stylesheet id="css-versions" href="/static/versions.css" />
		<div id="versions-page">
			<VersionDetailView />
		</div>
	}
}

#[component]
fn VersionDetailView() -> impl IntoView {
	let params = use_params_map();
	let version = move || params.read().get("version").unwrap_or_default();

	let version_detail = Resource::new(
		move || version(),
		|v| async move { get_version_detail(v).await },
	);

	view! {
		<Suspense fallback=|| view! { <div class="loading">"Loading version details..."</div> }>
			{move || {
				version_detail
					.get()
					.map(|data| match data {
						Ok(detail) => {
							view! {
								<div class="version-detail">
									<VersionHeader detail=detail.clone() />
									<VersionInfo detail=detail.clone() />
									<ArtifactsSection version=version() />
									<ChangelogSection detail=detail.clone() />
								</div>
							}
								.into_any()
						}
						Err(e) => {
							view! {
								<div class="error">{format!("Error loading version: {}", e)}</div>
							}
								.into_any()
						}
					})
			}}
		</Suspense>
	}
}

#[component]
fn VersionHeader(detail: VersionDetail) -> impl IntoView {
	view! {
		<div class="page-header">
			<h1>
				{detail.major} "." {detail.minor} "." {detail.patch}
			</h1>
		</div>
	}
}

#[component]
fn VersionInfo(detail: VersionDetail) -> impl IntoView {
	view! {
		<section class="detail-section">
			<div class="info-grid">
				<div class="info-item">
					<span class="info-label">"Created"</span>
					<span class="info-value">{detail.created_at.clone()}</span>
				</div>
				<div class="info-item">
					<span class="info-label">"Last updated"</span>
					<span class="info-value">{detail.updated_at.clone()}</span>
				</div>
				{detail.min_chrome_version.map(|chrome_ver| {
					view! {
						<div class="info-item">
							<span class="info-label">"Chrome support"</span>
							<span class="info-value">
								{format!("{} or later", chrome_ver)}
							</span>
						</div>
					}
				})}
			</div>
			<StatusSelection detail />
		</section>
	}
}

#[component]
fn StatusSelection(detail: VersionDetail) -> impl IntoView {
	let (selected_status, set_selected_status) = signal(detail.status);
	let (is_changing, set_is_changing) = signal(false);
	let version_str = format!("{}.{}.{}", detail.major, detail.minor, detail.patch);
	let can_switch_to_draft =
		detail.status != VersionStatus::Published || detail.is_latest_in_minor;

	let update_status = Action::new(move |new_status: &VersionStatus| {
		let version = version_str.clone();
		let status = new_status.to_string();
		async move { update_version_status(version, status).await }
	});

	let on_change = move |_| {
		set_is_changing.set(true);
	};

	let on_submit = move |ev: web_sys::SubmitEvent| {
		ev.prevent_default();
		update_status.dispatch(selected_status.get());
		set_is_changing.set(false);
	};

	Effect::new(move || {
		if let Some(Ok(())) = update_status.value().get() {
			window().location().reload().expect("Failed to reload page");
		}
	});

	view! {
		<form class="status-form" on:submit=on_submit>
			<select
				class="status-select"
				prop:value=move || selected_status.get().to_string()
				on:change=move |ev| {
					let value = event_target_value(&ev);
					let status = VersionStatus::from(value);
					set_selected_status.set(status);
					on_change(ev);
				}
			>
				<option
					value="draft"
					selected=move || selected_status.get() == VersionStatus::Draft
					disabled=!can_switch_to_draft
				>
					"Draft"
				</option>
				<option
					value="published"
					selected=move || selected_status.get() == VersionStatus::Published
				>
					"Published"
				</option>
				<option value="yanked" selected=move || selected_status.get() == VersionStatus::Yanked>
					"Yanked"
				</option>
			</select>
			<button
				type="submit"
				class="change-button"
				disabled=move || !is_changing.get() || update_status.pending().get()
			>
				{move || {
					if update_status.pending().get() {
						"Changing..."
					} else {
						"Change"
					}
				}}
			</button>
		</form>
		{move || {
			update_status
				.value()
				.get()
				.and_then(|result| {
					result
						.err()
						.map(|e| {
							view! { <div class="error-message">{format!("Error: {}", e)}</div> }
						})
				})
		}}
	}
}

#[component]
fn ArtifactsSection(version: String) -> impl IntoView {
	let artifacts = Resource::new(
		move || version.clone(),
		|v| async move { get_version_artifacts(v).await },
	);

	view! {
		<section class="detail-section">
			<h2>"Artifacts"</h2>
			<Suspense fallback=|| view! { <div class="loading">"Loading artifacts..."</div> }>
				{move || {
					artifacts
						.get()
						.map(|data| match data {
							Ok(artifacts) => {
								if artifacts.is_empty() {
									view! {
										<div class="no-artifacts">"No artifacts found for this version"</div>
									}
										.into_any()
								} else {
									view! {
										<div class="artifacts-list">
											<For each=move || artifacts.clone() key=|a| a.id let:artifact>
												<ArtifactItem artifact=artifact />
											</For>
										</div>
									}
										.into_any()
								}
							}
							Err(e) => {
								view! {
									<div class="error-message">{format!("Error loading artifacts: {}", e)}</div>
								}
									.into_any()
							}
						})
				}}
			</Suspense>
		</section>
	}
}

#[component]
fn ArtifactItem(artifact: ArtifactData) -> impl IntoView {
	view! {
		<div class="artifact-item">
			<div class="artifact-type">{artifact.artifact_type.clone()}</div>
			<div class="artifact-platform">{artifact.platform.clone()}</div>
			<a href={artifact.download_url.clone()} class="artifact-download" target="_blank">
				"Download"
			</a>
		</div>
	}
}

#[component]
fn ChangelogSection(detail: VersionDetail) -> impl IntoView {
	let (is_editing, set_is_editing) = signal(false);
	let (changelog_text, set_changelog_text) = signal(detail.changelog.clone());
	let version_str = format!("{}.{}.{}", detail.major, detail.minor, detail.patch);
	let original_changelog = StoredValue::new(detail.changelog.clone());

	let update_changelog = Action::new(move |new_changelog: &String| {
		let version = version_str.clone();
		let changelog = new_changelog.clone();
		async move { update_version_changelog(version, changelog).await }
	});

	Effect::new(move || {
		if let Some(Ok(())) = update_changelog.value().get() {
			window().location().reload().expect("Failed to reload page");
		}
	});

	view! {
		<section class="detail-section">
			{move || {
				if is_editing.get() {
					view! {
						<header>
							<h2>"Changelog"</h2>
						</header>
						<div class="changelog-editor">
							<textarea
								class="changelog-textarea"
								prop:value=move || changelog_text.get()
								on:input=move |ev| {
									set_changelog_text.set(event_target_value(&ev));
								}
							></textarea>
							<div class="changelog-actions">
								<button
									class="save-button"
									on:click=move |_| {
										update_changelog.dispatch(changelog_text.get());
										set_is_editing.set(false);
									}
									disabled=move || update_changelog.pending().get()
								>
									{move || {
										if update_changelog.pending().get() {
											"Saving..."
										} else {
											"Save"
										}
									}}
								</button>
								<button
									class="cancel-button"
									on:click=move |_| {
										set_changelog_text.set(original_changelog.get_value());
										set_is_editing.set(false);
									}
									disabled=move || update_changelog.pending().get()
								>
									"Cancel"
								</button>
							</div>
							{move || {
								update_changelog
									.value()
									.get()
									.and_then(|result| {
										result
											.err()
											.map(|e| {
												view! {
													<div class="error-message">{format!("Error: {}", e)}</div>
												}
											})
									})
							}}
						</div>
					}
						.into_any()
				} else {
					view! {
						<header>
							<h2>"Changelog"</h2>
							<button class="edit-button" on:click=move |_| set_is_editing.set(true)>
								"Edit"
							</button>
						</header>
						<div class="markdown-content" inner_html=parse_markdown(&detail.changelog)></div>
					}
						.into_any()
				}
			}}
		</section>
	}
}

fn parse_markdown(text: &str) -> String {
	use pulldown_cmark::{Options, Parser, html};

	let mut options = Options::empty();
	options.insert(Options::ENABLE_FOOTNOTES);
	options.insert(Options::ENABLE_GFM);
	options.insert(Options::ENABLE_SMART_PUNCTUATION);
	options.insert(Options::ENABLE_STRIKETHROUGH);
	options.insert(Options::ENABLE_TABLES);
	let parser = Parser::new_ext(text, options);
	let mut html_output = String::new();
	html::push_html(&mut html_output, parser);
	html_output
}
