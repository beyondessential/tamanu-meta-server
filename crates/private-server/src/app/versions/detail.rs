use leptos::prelude::*;
use leptos_meta::Stylesheet;
use leptos_router::hooks::use_params_map;

use crate::fns::versions::{
	ArtifactData, VersionDetail, get_version_artifacts, get_version_detail, update_artifact,
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

	let is_admin = Resource::new(
		|| (),
		|_| async { crate::fns::commons::is_current_user_admin().await },
	);

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
							let is_admin_result = is_admin.get().and_then(|r| r.ok()).unwrap_or(false);
							view! {
								<div class="version-detail">
									<VersionHeader detail=detail.clone() />
									<VersionInfo detail=detail.clone() />
									<StatusSection detail=detail.clone() is_admin=is_admin_result />
									<ArtifactsSection version=version() is_admin=is_admin_result />
									<ChangelogSection detail=detail.clone() is_admin=is_admin_result />
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
			<h2>"Information"</h2>
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
		</section>
	}
}

#[component]
fn StatusSection(detail: VersionDetail, is_admin: bool) -> impl IntoView {
	view! {
		<section class="detail-section">
			<h2>"Status"</h2>
			<StatusSelection detail is_admin />
		</section>
	}
}

#[component]
fn StatusSelection(detail: VersionDetail, is_admin: bool) -> impl IntoView {
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
		{if is_admin {
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
			}.into_any()
		} else {
			view! {
				<div class="status-display">
					<span class="status-value">{detail.status.to_string()}</span>
				</div>
			}.into_any()
		}}
	}
}

#[component]
fn ArtifactsSection(version: String, is_admin: bool) -> impl IntoView {
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
												<ArtifactItem artifact=artifact is_admin=is_admin />
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
fn ArtifactItem(artifact: ArtifactData, is_admin: bool) -> impl IntoView {
	let artifact_id = StoredValue::new(artifact.id);
	let original_type = StoredValue::new(artifact.artifact_type.clone());
	let original_platform = StoredValue::new(artifact.platform.clone());
	let original_url = StoredValue::new(artifact.download_url.clone());

	let (is_editing, set_is_editing) = signal(false);
	let (artifact_type, set_artifact_type) = signal(artifact.artifact_type.clone());
	let (platform, set_platform) = signal(artifact.platform.clone());
	let (download_url, set_download_url) = signal(artifact.download_url.clone());

	let update_artifact_action = Action::new(
		move |(id, art_type, plat, url): &(uuid::Uuid, String, String, String)| {
			let id = *id;
			let art_type = art_type.clone();
			let plat = plat.clone();
			let url = url.clone();
			async move { update_artifact(id, art_type, plat, url).await }
		},
	);

	Effect::new(move || {
		if let Some(Ok(())) = update_artifact_action.value().get() {
			window().location().reload().expect("Failed to reload page");
		}
	});

	view! {
		<div class="artifact-item">
			{move || {
				if is_editing.get() {
					view! {
						<div class="artifact-edit-form">
							<input
								type="text"
								class="artifact-input"
								placeholder="Type"
								prop:value=move || artifact_type.get()
								on:input=move |ev| {
									set_artifact_type.set(event_target_value(&ev));
								}
							/>
							<input
								type="text"
								class="artifact-input"
								placeholder="Platform"
								prop:value=move || platform.get()
								on:input=move |ev| {
									set_platform.set(event_target_value(&ev));
								}
							/>
							<input
								type="text"
								class="artifact-input artifact-url-input"
								placeholder="URL"
								prop:value=move || download_url.get()
								on:input=move |ev| {
									set_download_url.set(event_target_value(&ev));
								}
							/>
							<div class="artifact-edit-actions">
								<button
									class="save-button"
									on:click=move |_| {
										update_artifact_action
											.dispatch((
												artifact_id.get_value(),
												artifact_type.get(),
												platform.get(),
												download_url.get(),
											));
										set_is_editing.set(false);
									}

									disabled=move || update_artifact_action.pending().get()
								>
									{move || {
										if update_artifact_action.pending().get() {
											"Saving..."
										} else {
											"Save"
										}
									}}

								</button>
								<button
									class="cancel-button"
									on:click=move |_| {
										set_artifact_type.set(original_type.get_value());
										set_platform.set(original_platform.get_value());
										set_download_url.set(original_url.get_value());
										set_is_editing.set(false);
									}

									disabled=move || update_artifact_action.pending().get()
								>
									"Cancel"
								</button>
							</div>
							{move || {
								update_artifact_action
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
						<div class="artifact-type">{original_type.get_value()}</div>
						<div class="artifact-platform">{original_platform.get_value()}</div>
						<div class="artifact-url">{original_url.get_value()}</div>
						{if is_admin {
							view! {
								<button class="edit-button" on:click=move |_| set_is_editing.set(true)>
									"Edit"
								</button>
							}.into_any()
						} else {
							view! { <span></span> }.into_any()
						}}
					}
						.into_any()
				}
			}}

		</div>
	}
}

#[component]
fn ChangelogSection(detail: VersionDetail, is_admin: bool) -> impl IntoView {
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
			<details class="changelog-details" open>
				<summary>
					<h2>"Changelog"</h2>
				</summary>
				<div class="changelog-content">
					{move || {
						if is_editing.get() {
							view! {
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
								<div class="changelog-display">
									{if is_admin {
										view! {
											<button class="edit-button" on:click=move |_| set_is_editing.set(true)>
												"Edit"
											</button>
										}.into_any()
									} else {
										view! { <span></span> }.into_any()
									}}
									<div class="markdown-content" inner_html=parse_markdown(&detail.changelog)></div>
								</div>
							}
								.into_any()
						}
					}}
				</div>
			</details>
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
