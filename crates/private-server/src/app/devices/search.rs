use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

use super::list::DeviceList;

#[component]
pub fn Search() -> impl IntoView {
	let is_admin = Resource::new(
		|| (),
		|_| async {
			crate::fns::commons::is_current_user_admin()
				.await
				.unwrap_or(false)
		},
	);

	let (search_query, set_search_query) = signal(String::new());

	let search_results = Resource::new(
		move || search_query.get(),
		async |query| {
			if query.trim().is_empty() {
				Ok(vec![])
			} else {
				crate::fns::devices::search(query).await
			}
		},
	);

	view! {
		<div class="box mt-3">
			<h2 class="is-size-3">"Search devices"</h2>
			<div class="field">
				<div class="control">
					<input
						type="search"
						placeholder="Search by public key, key name, or connection IP…"
						prop:value=move || search_query.get()
						on:input=move |ev| set_search_query.set(event_target_value(&ev))
						class="input"
					/>
				</div>
			</div>
		</div>

		<Suspense fallback=|| view! { <progress class="progress is-small is-primary" max="100">"Loading..."</progress> }>
			{move || {
				let query = search_query.get();
				(!query.trim().is_empty()).then_some(()).and(search_results.get()).map(|result| {
					match result {
						Ok(devices) => {
							if devices.is_empty() {
								view! {
									<div class="box has-info-text">"No devices found matching your search"</div>
								}.into_any()
							} else {
								view! {
									<DeviceList devices />
								}.into_any()
							}
						}
						Err(e) => {
							view! {
								<div class="box has-danger-text">{format!("Search error: {}", e)}</div>
							}.into_any()
						}
					}
				})
			}}
		</Suspense>

		<Transition>
			{move || {
				is_admin.get().unwrap_or(false).then(|| view! { <ImportTicketForm /> })
			}}
		</Transition>
	}
}

#[component]
fn ImportTicketForm() -> impl IntoView {
	let (ticket, set_ticket) = signal(String::new());
	let (open, set_open) = signal(false);
	let (error, set_error) = signal(Option::<String>::None);

	let navigate = use_navigate();

	let do_import = Action::new(move |ticket_b64: &String| {
		let ticket_b64 = ticket_b64.clone();
		let navigate = navigate.clone();
		async move {
			let result = crate::fns::servers::import_ticket(ticket_b64).await;
			match result {
				Ok(server_id) => {
					set_open.set(false);
					set_ticket.set(String::new());
					set_error.set(None);
					navigate(&format!("/servers/{server_id}"), Default::default());
					Ok(server_id)
				}
				Err(e) => {
					set_error.set(Some(e.to_string()));
					Err(e)
				}
			}
		}
	});

	let on_submit = move |ev: web_sys::SubmitEvent| {
		ev.prevent_default();
		let value = ticket.get().trim().to_string();
		if value.is_empty() {
			set_error.set(Some("Ticket cannot be empty".to_string()));
			return;
		}
		set_error.set(None);
		do_import.dispatch(value);
	};

	view! {
		<section class="section pb-0">
			<button
				class="button is-primary"
				on:click=move |_| set_open.set(true)
			>
				"Import Ticket"
			</button>
		</section>

		<div class="modal" class:is-active=move || open.get()>
			<div class="modal-background" on:click=move |_| set_open.set(false) />
			<div class="modal-card">
				<header class="modal-card-head">
					<p class="modal-card-title">"Import Meta Ticket"</p>
					<button
						class="delete"
						aria-label="close"
						on:click=move |_| set_open.set(false)
					/>
				</header>
				<form on:submit=on_submit>
					<section class="modal-card-body">
						<div class="field">
							<label class="label">"Ticket (base64)"</label>
							<div class="control">
								<textarea
									class="textarea is-family-monospace"
									rows="5"
									placeholder="Paste the base64-encoded Meta Ticket here..."
									prop:value=move || ticket.get()
									on:input=move |ev| set_ticket.set(event_target_value(&ev))
								/>
							</div>
						</div>
						{move || error.get().map(|e| view! {
							<p class="help is-danger">{e}</p>
						})}
					</section>
					<footer class="modal-card-foot">
						<div class="buttons">
							<button
								type="submit"
								class="button is-primary"
								disabled=move || do_import.pending().get()
							>
								{move || if do_import.pending().get() { "Importing..." } else { "Import" }}
							</button>
							<button
								type="button"
								class="button"
								on:click=move |_| set_open.set(false)
								disabled=move || do_import.pending().get()
							>
								"Cancel"
							</button>
						</div>
					</footer>
				</form>
			</div>
		</div>
	}
}
