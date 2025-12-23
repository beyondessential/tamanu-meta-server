use std::sync::Arc;

use leptos::prelude::*;

use crate::{
	components::{ErrorHandler, LoadingBar, Nothing, PaginatedList},
	fns::bestool::{count_snippets, create_snippet, list_snippets},
};

pub mod detail;

const PAGE_SIZE: u64 = 10;

#[component]
pub fn List() -> impl IntoView {
	let count = Resource::new(
		|| (),
		|_| async { count_snippets().await.unwrap_or_default() },
	);

	let fetcher = move |p| async move {
		let offset = p * PAGE_SIZE;
		list_snippets(offset, Some(PAGE_SIZE)).await
	};

	let (page, set_page) = signal(0u64);
	let snippets = Resource::new(move || page.get(), fetcher);

	let (show_create, set_show_create) = signal(false);

	let refresh = move || {
		snippets.refetch();
		count.refetch();
	};

	view! {
		<header class="level mt-4">
			<div class="level-left">
				<h1 class="level-item is-size-3">PSQL Snippets</h1>
			</div>
			<div class="level-right">
				<button
					class="button"
					class:is-info={move || !show_create.get()}
					class:is-danger={move || show_create.get()}
					on:click=move |_| set_show_create.set(!show_create.get())
				>
					{move || if show_create.get() { "Cancel" } else { "Add" }}
				</button>
			</div>
		</header>
		{move || show_create.get().then(|| {
			view! { <CreateSnippetForm on_created=move || {
				refresh();
				set_show_create.set(false);
			} /> }
		})}
		<Transition fallback=|| view! { <LoadingBar /> }>
			<ErrorHandler>
				{move || {
					match snippets.get() {
						Some(Ok(snippets_list)) => {
							if snippets_list.is_empty() {
								view! {
									<Nothing thing="snippets" />
								}.into_any()
							} else {
								view! {
									<PaginatedList
										page=page
										set_page=set_page
										total_count=Signal::derive(move || count.get().unwrap_or(0))
										page_size=PAGE_SIZE
									>
										<SnippetList snippets=snippets_list />
									</PaginatedList>
								}.into_any()
							}
						}
						_ => view! {}.into_any(),
					}
				}}
			</ErrorHandler>
		</Transition>
	}
}

#[component]
fn CreateSnippetForm<F>(on_created: F) -> impl IntoView
where
	F: Fn() + 'static + Send + Sync,
{
	let on_created = Arc::new(on_created);
	let (name, set_name) = signal(String::new());
	let (description, set_description) = signal(String::new());
	let (sql, set_sql) = signal(String::new());

	let create_action = Action::new(move |_: &()| {
		let name_val = name.get();
		let desc_val = if description.get().is_empty() {
			None
		} else {
			Some(description.get())
		};
		let sql_val = sql.get();
		let on_created = Arc::clone(&on_created);
		async move {
			match create_snippet(name_val, desc_val, sql_val).await {
				Ok(_) => {
					set_name.set(String::new());
					set_description.set(String::new());
					set_sql.set(String::new());
					on_created();
					Ok(())
				}
				Err(e) => Err(e),
			}
		}
	});

	view! {
		<div class="box">
			<form class="field" on:submit=move |ev| {
				ev.prevent_default();
				create_action.dispatch(());
			}>
				<div class="field">
					<label class="label">"Name (will become the filename/snipname)"</label>
					<div class="control">
						<input
							class="input"
							type="text"
							required
							placeholder="example_snippet_name"
							disabled=move || create_action.pending().get()
							prop:value=move || name.get()
							on:input=move |ev| set_name.set(event_target_value(&ev))
						/>
					</div>
				</div>

				<div class="field">
					<label class="label">"Description (optional)"</label>
					<div class="control">
						<textarea
							class="textarea"
							placeholder="Details about what it does, variables required, etc. Currently not displayed in bestool but that may change."
							disabled=move || create_action.pending().get()
							prop:value=move || description.get()
							on:input=move |ev| set_description.set(event_target_value(&ev))
						></textarea>
					</div>
				</div>

				<div class="field">
					<label class="label">"SQL (no sensitive info! everything here may be read by anyone with bestool)"</label>
					<div class="control">
						<textarea
							class="textarea monospace"
							required
							placeholder="SELECT ..."
							disabled=move || create_action.pending().get()
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
							disabled=move || create_action.pending().get()
							class:is-loading=move || create_action.pending().get()
						>
							"Save"
						</button>
					</div>
				</div>
			</form>
		</div>
	}
}

#[component]
fn SnippetList(snippets: Vec<Arc<crate::fns::bestool::BestoolSnippetInfo>>) -> impl IntoView {
	view! {
		<For each=move || snippets.clone() key=|snippet| snippet.id let:snippet>
			<a href=format!("/bestool/snippets/{}", snippet.id) class="box">
				<p class="monospace">{snippet.name.clone()}</p>
				<p class="snippet-description">{snippet.description.clone()}</p>
			</a>
		</For>
	}
}
