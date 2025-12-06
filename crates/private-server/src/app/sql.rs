use leptos::prelude::*;
use leptos_meta::Stylesheet;
use serde_json::Value;

use crate::components::{ErrorHandler, LoadingBar, PaginatedList, TimeAgo};
use crate::fns::sql::{
	SqlHistoryEntry, SqlQuery, SqlResult, execute_query, get_last_user_query, get_query_history,
	get_query_history_count,
};

#[component]
pub fn Page() -> impl IntoView {
	view! {
		<Stylesheet id="css-sql" href="/static/private/sql.css" />
		<section class="section" id="sql-page">
			<h1 class="title">"SQL Playground"</h1>
			<Suspense>
				<ErrorHandler>
					<SqlQueryForm />
				</ErrorHandler>
			</Suspense>
		</section>
	}
}

#[component]
pub fn SqlQueryForm() -> impl IntoView {
	let (query, set_query) = signal(String::new());
	let (is_executing, set_is_executing) = signal(false);
	let (error, set_error) = signal::<Option<String>>(None);
	let (result, set_result) = signal::<Option<SqlResult>>(None);
	let (show_history, set_show_history) = signal(false);
	let (history_page, set_history_page) = signal(0);

	let last_query_action = Action::new(|_| async move {
		match get_last_user_query().await {
			Ok(Some(last_query)) => last_query,
			Ok(None) => String::new(),
			Err(_) => String::new(),
		}
	});

	let history_count =
		LocalResource::new(|| async move { get_query_history_count().await.unwrap_or(0) });
	let history_resource = LocalResource::new(move || async move {
		let page_size = 10;
		let offset = history_page.get() * page_size;
		get_query_history(offset, Some(page_size)).await.ok()
	});

	let load_last_query = move |_| {
		last_query_action.dispatch(());
	};

	Effect::new(move |_| {
		if let Some(last_query) = last_query_action.value().get() {
			if !last_query.is_empty() {
				set_query.set(last_query);
			}
		}
	});

	let toggle_history = move |_| {
		set_show_history.update(|v| *v = !*v);
	};

	let execute_action = Action::new(move |query_text: &String| {
		let query_text = query_text.clone();
		async move {
			set_is_executing.set(true);
			set_error.set(None);
			set_result.set(None);

			let query = SqlQuery {
				query: query_text.clone(),
			};

			match execute_query(query).await {
				Ok(result) => {
					set_result.set(Some(result));
				}
				Err(err) => {
					set_error.set(Some(err.to_string()));
				}
			}

			set_is_executing.set(false);
		}
	});

	let handle_submit = move |ev: leptos::ev::SubmitEvent| {
		ev.prevent_default();
		if !query.get().trim().is_empty() {
			execute_action.dispatch(query.get());
		}
	};

	view! {
		<div class="sql-query-container">
			<form on:submit=handle_submit>
				<div class="field">
					<div class="control">
						<textarea
							class="textarea monospace"
							placeholder="SELECT * FROM statuses ORDER BY created_at DESC LIMIT 10;"
							rows=10
							prop:value=query
							disabled=move || is_executing.get()
							on:input=move |ev| set_query.set(event_target_value(&ev))
							on:keyup=move |ev| {
								if ev.key() == "Enter" && ev.ctrl_key() {
									if !query.get().trim().is_empty() {
										execute_action.dispatch(query.get());
									}
								}
							}
						/>
					</div>
				</div>

				<div class="field is-grouped">
					<div class="control">
						<button
							class="button is-primary"
							type="submit"
							disabled=move || query.get().trim().is_empty() || is_executing.get()
							class:is-loading=move || is_executing.get()
						>
							<span>
								"Run"
							</span>
						</button>
					</div>
					<div class="control is-expanded"></div>
					<div class="control">
						<button
							class="button is-info"
							type="button"
							on:click=load_last_query
							disabled=move || is_executing.get()
						>
							<span class="icon">
								"↶"
							</span>
							<span>"Last query"</span>
						</button>
					</div>
					<div class="control">
						<button
							class="button is-info is-light"
							type="button"
							on:click=toggle_history
							disabled=move || is_executing.get()
						>
							<span>"History"</span>
							<span class="icon">
								{move || if show_history.get() { "▼" } else { "▶" }}
							</span>
						</button>
					</div>
				</div>
			</form>

			<Show when=move || show_history.get()>
				<Transition fallback=move || view! { <LoadingBar /> }>
					{move || history_resource.get().map(|history| match history {
						Some(data) => view! {
							<div class="box my-4">
								<PaginatedList
									page=history_page
									set_page=set_history_page
									total_count=Signal::derive(move || history_count.get().unwrap_or(0))
									page_size=10
								>
									<HistoryDisplay data set_query />
								</PaginatedList>
							</div>
						}.into_any(),
						None => view! { <div class="notification is-warning">"No history available"</div> }.into_any(),
					})}
				</Transition>
			</Show>

			<Show when=move || error.get().is_some()>
				<div class="notification is-danger mt-4">
					<button class="delete" on:click=move |_| set_error.set(None)></button>
					<strong>"Error executing query:"</strong>
					<br />
					{error.get().unwrap()}
				</div>
			</Show>

			<Show when=move || result.get().is_some()>
				<SqlResultDisplay result=result.get().unwrap() />
			</Show>
		</div>
	}
}

#[component]
pub fn SqlResultDisplay(result: SqlResult) -> impl IntoView {
	view! {
		<div class="sql-result-container mt-6">
			<div class="notification is-info is-light">
				<p>
					<strong>"Query executed successfully!"</strong>
					<br />
					<span>"Returned "</span>
					<strong>{result.row_count}</strong>
					<span>" rows in "</span>
					<strong>{result.execution_time_ms}</strong>
					<span>" ms"</span>
				</p>
			</div>

			<div class="table-container">
				<table class="table is-bordered is-striped is-narrow is-hoverable is-fullwidth">
					<thead>
						<tr>
							<For
								each=move || result.columns.clone()
								key=|col| col.clone()
								let:column
							>
								<th>{column}</th>
							</For>
						</tr>
					</thead>
					<tbody>
						<For
							each=move || result.rows.clone()
							key=|row| format!("{:?}", row)
							let:row
						>
							<tr>
								<For
									each=move || row.clone()
									key=|cell| format!("{:?}", cell)
									let:cell
								>
									<td>
										<JsonValueDisplay value=cell />
									</td>
								</For>
							</tr>
						</For>
					</tbody>
				</table>
			</div>

			<Show when=move || result.row_count == 0>
				<div class="notification is-warning mt-4">
					"No rows returned by the query."
				</div>
			</Show>
		</div>
	}
}

#[component]
pub fn JsonValueDisplay(value: Value) -> impl IntoView {
	match value {
		Value::Null => view! { <span class="has-text-grey">"NULL"</span> }.into_any(),
		Value::Bool(b) => view! { <span class="has-text-info">{b.to_string()}</span> }.into_any(),
		Value::Number(n) => {
			view! { <span class="has-text-primary">{n.to_string()}</span> }.into_any()
		}
		Value::String(s) => view! { <span class="has-text-success">{s}</span> }.into_any(),
		Value::Array(arr) => view! {
			<span class="has-text-warning">
				"["
				{arr.iter().map(|v| view! { <JsonValueDisplay value=v.clone() /> }).collect::<Vec<_>>()}
				"]"
			</span>
		}
		.into_any(),
		obj @ Value::Object(_) => view! {
			<pre>{serde_json::to_string_pretty(&obj).unwrap_or_default()}</pre>
		}
		.into_any(),
	}
}

#[component]
pub fn HistoryDisplay(data: Vec<SqlHistoryEntry>, set_query: WriteSignal<String>) -> impl IntoView {
	view! {
		<table class="table is-bordered is-striped is-narrow is-hoverable is-fullwidth">
			<thead>
				<tr>
					<th>"When"</th>
					<th>"Who"</th>
					<th>"What"</th>
					<th></th>
				</tr>
			</thead>
			<tbody>
				<For
					each=move || data.clone()
					key=|entry| entry.id
					let:entry
				>
					<tr>
						<td>
							<TimeAgo timestamp={entry.created_at} />
						</td>
						<td>
							<span class="tag is-info">{entry.tailscale_user}</span>
						</td>
						<td class="monospace">
							{entry.query.clone()}
						</td>
						<td>
							<button class="button is-small is-info" on:click=move |_| set_query.set(entry.query.clone())>
								Recall
							</button>
						</td>
					</tr>
				</For>
			</tbody>
		</table>
	}
}
