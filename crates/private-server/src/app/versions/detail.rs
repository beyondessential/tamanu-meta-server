use commons_errors::AppError;
use leptos::prelude::*;
use leptos_meta::Stylesheet;
use leptos_router::hooks::use_params_map;
use uuid::Uuid;

use crate::{
	components::{ErrorHandler, LoadingBar, TimeAgo, ToggleSignal as _},
	fns::versions::{
		ArtifactData, RelatedVersionData, VersionDetail, create_artifact, delete_artifact,
		get_artifacts_by_version_id, get_version_detail, update_artifact, update_version_changelog,
		update_version_status,
	},
};
use commons_types::version::VersionStatus;

#[component]
pub fn Detail() -> impl IntoView {
	view! {
		<Stylesheet id="css-versions" href="/static/versions.css" />
		<VersionDetailView />
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

	let version_detail = Resource::new(version, |v| async move { get_version_detail(v).await });

	view! {
		<Transition fallback=|| view! { <LoadingBar /> }>
			<ErrorHandler>
				{move || version_detail.and_then(|detail| {
					let is_admin = is_admin.get().and_then(|r| r.ok()).unwrap_or(false);
					view! {
						<header class="level mt-4">
							<div class="level-left">
								<h1 class="level-item is-size-3">{detail.major} "." {detail.minor} "." {detail.patch}</h1>
							</div>
							<div class="level-right">
								<StatusSelection detail=detail.clone() is_admin {..} class:level-item />
							</div>
						</header>
						<VersionInfo detail=detail.clone() />
						<ArtifactsSection version_id=detail.id is_admin />
						<ChangelogSection detail=detail.clone() is_admin />
						{(!detail.related_versions.is_empty()).then(|| {
							view! { <RelatedVersionsSection related_versions=detail.related_versions.clone() /> }
						})}
					}
				})}
			</ErrorHandler>
		</Transition>
	}
}

#[component]
fn VersionInfo(detail: VersionDetail) -> impl IntoView {
	view! {
		<section class="box">
			<div class="info-grid">
				<div class="info-item">
					<span class="info-label">"Created"</span>
					<span class="info-value">{detail.created_at.strftime("%Y-%m-%d").to_string()}</span>
				</div>
				<div class="info-item">
					<span class="info-label">"Last updated"</span>
					<TimeAgo timestamp={detail.updated_at} {..} class:info-value />
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
		<form class="field" class:has-addons=is_admin on:submit=on_submit>
			<div class="control">
				<div class="select">
					<select
						disabled={!is_admin}
						prop:value=move || selected_status.get()
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
				</div>
			</div>
			{is_admin.then(|| view! {
				<div class="control">
					<button
						type="submit"
						class="button is-primary"
						disabled=move || { !is_changing.get() || update_status.pending().get() }
					>
						{move || {
							if update_status.pending().get() {
								"Changing..."
							} else {
								"Change"
							}
						}}
					</button>
				</div>
			})}
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
fn ArtifactsSection(version_id: Uuid, is_admin: bool) -> impl IntoView {
	let (is_unlocked, set_is_unlocked) = signal(false);
	let (show_create, set_show_create) = signal(false);

	let resource = Resource::new(
		move || version_id,
		|id| async move { get_artifacts_by_version_id(id).await },
	);
	let refresh = move || resource.refetch();

	view! {
		<header class="level">
			<div class="level-left">
				<h2 class="level-item is-size-4">"Artifacts"</h2>
			</div>
			{is_admin.then(|| {
				view! {
					<div class="level-right">
						{move || is_unlocked.get().then(|| {
							view! {
								<button
									class="level-item button"
									on:click=move |_| set_show_create.toggle()
									class:is-warning=move || show_create.get()
									class:is-primary=move || {!show_create.get()}
								>
									{move || if show_create.get() { "Cancel create" } else { "Create" }}
								</button>
							}
						})}
						<button class="level-item button" on:click=move |_| set_is_unlocked.toggle()>
							{move || if is_unlocked.get() { "Lock" } else { "Unlock" }}
						</button>
					</div>
				}
			})}
		</header>
		{move || show_create.get().then(|| {
			view! { <CreateArtifactForm version_id refresh=move || {
				refresh();
				set_show_create.set(false);
			} /> }
		})}
		<ArtifactsList resource is_unlocked />
	}
}

#[component]
fn ArtifactsList(
	resource: Resource<Result<Vec<ArtifactData>, AppError>>,
	is_unlocked: ReadSignal<bool>,
) -> impl IntoView {
	view! {
		<Transition fallback=|| view! { <LoadingBar /> }>
			{move || resource.and_then(|artifacts| {
				if artifacts.is_empty() {
					view! {
						<div class="no-artifacts">"No artifacts found for this version"</div>
					}.into_any()
				} else {
					let artifacts = artifacts.clone();
					view! {
						<For each=move || artifacts.clone() key=|a| a.id let:artifact>
							<ArtifactItem artifact is_unlocked refresh={move || resource.refetch()} />
						</For>
					}.into_any()
				}
			})
		}</Transition>
	}
}

#[component]
fn CreateArtifactForm(
	version_id: Uuid,
	refresh: impl Fn() + Send + Sync + Copy + 'static,
) -> impl IntoView {
	let (artifact_type, set_artifact_type) = signal(String::new());
	let (platform, set_platform) = signal(String::new());
	let (download_url, set_download_url) = signal(String::new());

	let create_action = Action::new(move |_: &()| async move {
		let _ = create_artifact(
			version_id,
			artifact_type.get(),
			platform.get(),
			download_url.get(),
		)
		.await;
		refresh();
	});

	view! {
		<form class="field is-grouped" on:submit=move |ev| {
			ev.prevent_default();
			create_action.dispatch(());
		}>
			<p class="control">
				<input
					class="input"
					type="text"
					required
					placeholder="Type"
					disabled=move || create_action.pending().get()
					prop:value=move || artifact_type.get()
					on:input=move |ev| set_artifact_type.set(event_target_value(&ev)) />
			</p>
			<p class="control">
				<input
					class="input"
					type="text"
					required
					placeholder="Platform"
					disabled=move || create_action.pending().get()
					prop:value=move || platform.get()
					on:input=move |ev| set_platform.set(event_target_value(&ev)) />
			</p>
			<p class="control is-expanded">
				<input
					class="input"
					type="text"
					required
					placeholder="Download URL"
					disabled=move || create_action.pending().get()
					prop:value=move || download_url.get()
					on:input=move |ev| set_download_url.set(event_target_value(&ev)) />
			</p>
			<p class="control">
				<button
					type="submit"
					class="button is-primary"
					disabled=move || create_action.pending().get()
					class:is-loading=move || create_action.pending().get()
				>"Create"</button>
			</p>
		</form>
	}
}

#[component]
fn ArtifactItem(
	artifact: ArtifactData,
	is_unlocked: ReadSignal<bool>,
	refresh: impl Fn() + Send + Sync + Copy + 'static,
) -> impl IntoView {
	let (is_editing, set_is_editing) = signal(false);

	let delete_action = Action::new(move |id: &Uuid| {
		let id = *id;
		async move {
			let _ = delete_artifact(id).await;
			refresh();
		}
	});

	view! {
		{move || if is_editing.get() {
			let artifact = artifact.clone();
			view! { <ArtifactItemEdit artifact set_is_editing /> }.into_any()
		} else {
			let artifact = artifact.clone();
			view! { <ArtifactItemView artifact is_unlocked delete_action set_is_editing /> }.into_any()
		}}
	}
}

#[component]
fn ArtifactItemView(
	artifact: ArtifactData,
	is_unlocked: ReadSignal<bool>,
	delete_action: Action<Uuid, ()>,
	set_is_editing: WriteSignal<bool>,
) -> impl IntoView {
	let (show_delete_confirm, set_show_delete_confirm) = signal(false);

	view! {
		<div class="box mb-3">
			<div class="columns">
				<div class="column">{artifact.artifact_type.clone()}</div>
				<div class="column">{artifact.platform.clone()}</div>
				<a
					class="column is-half"
					href={artifact.download_url.starts_with("https://").then(|| artifact.download_url.clone())}
					class:has-text-primary-dark={!artifact.download_url.starts_with("https://")}
				>{artifact.download_url.clone()}</a>
				<div class="column">
					<div class="field is-grouped buttons are-small is-justify-content-end" class:is-invisible={move || !is_unlocked.get()}>
					{move || if show_delete_confirm.get() {
						view! {
							<p class="control">
								<button
									class="button is-danger"
									on:click=move |_| drop(delete_action.dispatch(artifact.id))
									disabled=move || delete_action.pending().get()
									class:is-loading=move || delete_action.pending().get()
								>"Really delete"</button>
							</p>
							<p class="control">
								<button
									class="button is-light"
									on:click=move |_| set_show_delete_confirm.set(false)
									disabled=move || delete_action.pending().get()
								>"Cancel"</button>
							</p>
						}.into_any()
					} else {
						view! {
							<p class="control">
								<button
									class="button is-info"
									on:click=move |_| set_is_editing.set(true)
								>"Edit"</button>
							</p>
							<p class="control">
								<button
									class="button is-danger"
									on:click=move |_| set_show_delete_confirm.set(true)
								>"Delete"</button>
							</p>
						}.into_any()
					}}
					</div>
				</div>
			</div>
		</div>
	}
}

#[component]
fn ArtifactItemEdit(artifact: ArtifactData, set_is_editing: WriteSignal<bool>) -> impl IntoView {
	let (artifact_type, set_artifact_type) = signal(artifact.artifact_type.clone());
	let (platform, set_platform) = signal(artifact.platform.clone());
	let (download_url, set_download_url) = signal(artifact.download_url.clone());

	let update_action = Action::new(move |_: &()| async move {
		let _ = update_artifact(
			artifact.id,
			artifact_type.get(),
			platform.get(),
			download_url.get(),
		)
		.await;
		set_is_editing.set(false);
	});

	view! {
		<div class="box">
			<form class="field is-grouped" on:submit=move |ev| {
				ev.prevent_default();
				update_action.dispatch(());
			}>
				<p class="control">
					<input
						class="input"
						type="text"
						required
						placeholder="Type"
						disabled=move || update_action.pending().get()
						prop:value=move || artifact_type.get()
						on:input=move |ev| set_artifact_type.set(event_target_value(&ev)) />
				</p>
				<p class="control">
					<input
						class="input"
						type="text"
						required
						placeholder="Platform"
						disabled=move || update_action.pending().get()
						prop:value=move || platform.get()
						on:input=move |ev| set_platform.set(event_target_value(&ev)) />
				</p>
				<p class="control is-expanded">
					<input
						class="input"
						type="text"
						required
						placeholder="Download URL"
						disabled=move || update_action.pending().get()
						prop:value=move || download_url.get()
						on:input=move |ev| set_download_url.set(event_target_value(&ev)) />
				</p>
				<p class="control">
					<button
						type="submit"
						class="button is-primary"
						class:is-loading=move || update_action.pending().get()
					>"Save"</button>
				</p>
				<p class="control">
					<button
						class="button is-danger is-light"
						disabled=move || update_action.pending().get()
						on:click=move |_| set_is_editing.set(false)
					>"Cancel"</button>
				</p>
			</form>
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
		<header class="level mt-4">
			<div class="level-left">
				<h2 class="level-item is-size-4">"Changelog"</h2>
			</div>
			{is_admin.then(|| {
				view! {
					<div class="level-right">
						{move || if is_editing.get() {
							view! {
								<button
									class="level-item button is-success mr-2"
									on:click=move |_| {
										update_changelog.dispatch(changelog_text.get());
										set_is_editing.set(false);
									}
								>"Save"</button>
								<button
									class="level-item button is-danger is-light"
									on:click=move |_| {
										set_changelog_text.set(original_changelog.get_value());
										set_is_editing.set(false);
									}
								>"Cancel"</button>
							}.into_any()
						} else {
							view! {
								<button
									class="level-item button is-info"
									on:click=move |_| {
										set_is_editing.set(true);
									}
								>"Edit"</button>
							}.into_any()
						}}
					</div>
				}
			})}
		</header>
		<section class="box">
			{move || {
				if is_editing.get() {
					view! {
						<textarea
							class="textarea monospace"
							rows="20"
							prop:value=move || changelog_text.get()
							on:input=move |ev| set_changelog_text.set(event_target_value(&ev))
						></textarea>
					}.into_any()
				} else {
					view! {
						<div class="content" inner_html=parse_markdown(&detail.changelog)></div>
					}.into_any()
				}
			}}
		</section>
	}
}

#[component]
fn RelatedVersionsSection(related_versions: Vec<RelatedVersionData>) -> impl IntoView {
	view! {
		{related_versions
			.into_iter()
			.map(|related| {
				view! {
					<h4 class="is-size-5">
						{related.major}
						"."
						{related.minor}
						"."
						{related.patch}
					</h4>
					<div class="box content" inner_html=parse_markdown(&related.changelog) />
				}
			})
			.collect_view()}
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
