use leptos::prelude::*;
use leptos_meta::Stylesheet;
use serde_json::Value;

use crate::components::ErrorHandler;
use crate::fns::sql::{SqlQuery, SqlResult, execute_query};

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

	let handle_clear = move |_| {
		set_query.set(String::new());
		set_error.set(None);
		set_result.set(None);
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
						>
							<span class="icon">
								"▶"
							</span>
							<span>
								{move || if is_executing.get() { "Executing..." } else { "Execute Query" }}
							</span>
						</button>
					</div>
					<div class="control">
						<button
							class="button is-light"
							type="button"
							on:click=handle_clear
							disabled=move || is_executing.get()
						>
							<span class="icon">
								"✕"
							</span>
							<span>"Clear"</span>
						</button>
					</div>
				</div>
			</form>

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
		Value::Object(obj) => view! {
			<span class="has-text-warning">
				"{"
				{format!("{} fields", obj.len())}
				"}"
			</span>
		}
		.into_any(),
	}
}
