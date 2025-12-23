use leptos::prelude::*;
use leptos_router::NavigateOptions;
use leptos_router::hooks::{use_navigate, use_params_map};
use uuid::Uuid;

use crate::{
	components::{ErrorHandler, LoadingBar},
	fns::bestool::{BestoolSnippetDetail, get_latest_snippet_id, get_snippet, update_snippet},
};

#[component]
pub fn Detail() -> impl IntoView {
	let params = use_params_map();
	let snippet_id = move || {
		params
			.read()
			.get("id")
			.and_then(|id| Uuid::parse_str(&id).ok())
	};

	let snippet = Resource::new(snippet_id, |id| async move {
		if let Some(id) = id {
			get_snippet(id).await.ok()
		} else {
			None
		}
	});

	// Check if this snippet is superseded and redirect to latest if needed
	let navigate = use_navigate();
	let _ = LocalResource::new(move || {
		let navigate = navigate.clone();
		async move {
			if let Some(Some(detail)) = snippet.get() {
				let current_id = detail.id;
				if let Ok(latest_id) = get_latest_snippet_id(current_id).await {
					if latest_id != current_id {
						let mut opts = NavigateOptions::default();
						opts.replace = true;
						navigate(&format!("/bestool/snippets/{}", latest_id), opts);
					}
				}
			}
		}
	});

	view! {
		<Transition fallback=|| view! { <LoadingBar /> }>
			<ErrorHandler>
				{move || {
					snippet.get().flatten().map(|detail| {
						view! { <SnippetDetailView detail=detail.clone() /> }
					})
				}}
			</ErrorHandler>
		</Transition>
	}
}

#[component]
fn SnippetDetailView(detail: BestoolSnippetDetail) -> impl IntoView {
	let (is_editing, set_is_editing) = signal(false);
	let (name, set_name) = signal(detail.name.clone());
	let (description, set_description) = signal(detail.description.clone().unwrap_or_default());
	let (sql, set_sql) = signal(detail.sql.clone());

	let navigate = use_navigate();

	let update_action = Action::new(move |_: &()| {
		let id = detail.id;
		let name_val = name.get();
		let desc_val = if description.get().is_empty() {
			None
		} else {
			Some(description.get())
		};
		let sql_val = sql.get();
		let nav_fn = navigate.clone();
		async move {
			match update_snippet(id, name_val, desc_val, sql_val).await {
				Ok(new_detail) => {
					set_is_editing.set(false);
					// Navigate to the new version with replace
					let mut opts = NavigateOptions::default();
					opts.replace = true;
					nav_fn(&format!("/bestool/snippets/{}", new_detail.id), opts);
					Ok(())
				}
				Err(e) => Err(e),
			}
		}
	});

	view! {
		<section class="section">
			{move || {
				if is_editing.get() {
					view! {
						<div class="level">
							<h1 class="level-item is-size-3">Edit</h1>
						</div>
						<div class="box">
							<form class="field" on:submit=move |ev| {
								ev.prevent_default();
								update_action.dispatch(());
							}>
								<div class="field">
									<label class="label">"Name"</label>
									<div class="control">
										<input
											class="input"
											type="text"
											required
											placeholder="example_snippet_name"
											disabled=move || update_action.pending().get()
											prop:value=move || name.get()
											on:input=move |ev| set_name.set(event_target_value(&ev))
										/>
									</div>
								</div>

								<div class="field">
									<label class="label">"Description"</label>
									<div class="control">
										<textarea
											class="textarea"
											placeholder="Optional description"
											disabled=move || update_action.pending().get()
											prop:value=move || description.get()
											on:input=move |ev| set_description.set(event_target_value(&ev))
										></textarea>
									</div>
								</div>

								<div class="field">
									<label class="label">"SQL"</label>
									<div class="control">
										<textarea
											class="textarea monospace"
											required
											placeholder="SELECT ..."
											disabled=move || update_action.pending().get()
											prop:value=move || sql.get()
											on:input=move |ev| set_sql.set(event_target_value(&ev))
										></textarea>
									</div>
								</div>

								<div class="field is-grouped">
									<div class="control">
										<button
											type="submit"
											class="button is-primary"
											disabled=move || update_action.pending().get()
											class:is-loading=move || update_action.pending().get()
										>
											"Save"
										</button>
									</div>
									<div class="control">
										<button
											type="button"
											class="button is-danger is-light"
											on:click=move |_| set_is_editing.set(false)
										>
											"Cancel"
										</button>
									</div>
								</div>
							</form>
						</div>
					}.into_any()
				} else {
					view! {
						<div class="level">
							<div class="level-left">
								<h1 class="level-item is-size-3 monospace">{detail.name.clone()}</h1>
							</div>
							<div class="level-right">
								<div class="level-item">
									"Last edit by "
									{detail.editor.clone()}
								</div>
								<div class="level-item">
									<button
										class="button is-primary"
										on:click=move |_| set_is_editing.set(true)
									>
										"Edit"
									</button>
								</div>
							</div>
						</div>
						<div class="box">
							{detail.description.clone().map(|desc| {
								view! { <p class="block">{desc}</p> }
							})}
							<pre class="block" style="white-space: break-spaces">
								<code>{detail.sql.clone()}</code>
							</pre>
						</div>
					}.into_any()
				}
			}}
		</section>
	}
}
