use commons_types::server::{MetaTicket, kind::ServerKind, rank::ServerRank};
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
			<header class="level">
				<div class="level-left">
					<h2 class="level-item is-size-4">"Search devices"</h2>
				</div>
				<Transition>
					{move || {
						is_admin.get().unwrap_or(false).then(|| view! {
							<div class="level-right">
								<ImportTicketForm />
							</div>
						})
					}}
				</Transition>
			</header>
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
	}
}

#[component]
fn ImportTicketForm() -> impl IntoView {
	let (ticket, set_ticket) = signal(String::new());
	let (kind, set_kind) = signal(ServerKind::Facility);
	let (rank, set_rank) = signal(Option::<ServerRank>::None);

	// Parse the ticket client-side to extract any kind/rank hints baked into it.
	let parsed_ticket = Memo::new(move |_| MetaTicket::from_base64(&ticket.get()).ok());

	let kind_from_ticket = Memo::new(move |_| parsed_ticket.get()?.kind);

	let rank_from_ticket = Memo::new(move |_| parsed_ticket.get()?.rank);

	// When the ticket supplies kind/rank, override the user's selection.
	Effect::new(move |_| {
		if let Some(k) = kind_from_ticket.get() {
			set_kind.set(k);
		}
		if let Some(r) = rank_from_ticket.get() {
			set_rank.set(Some(r));
		}
	});
	let (open, set_open) = signal(false);
	let (error, set_error) = signal(Option::<String>::None);

	let navigate = use_navigate();

	let do_import = Action::new(
		move |(ticket_b64, kind, rank): &(String, ServerKind, Option<ServerRank>)| {
			let ticket_b64 = ticket_b64.clone();
			let kind = *kind;
			let rank = *rank;
			async move { crate::fns::servers::import_ticket(ticket_b64, kind, rank).await }
		},
	);

	Effect::new(move |_| {
		if let Some(result) = do_import.value().get() {
			match result {
				Ok(server_id) => {
					set_open.set(false);
					set_ticket.set(String::new());
					set_error.set(None);
					navigate(&format!("/servers/{server_id}"), Default::default());
				}
				Err(e) => {
					set_error.set(Some(e.to_string()));
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
		do_import.dispatch((value, kind.get(), rank.get()));
	};

	view! {
		<button
			class="button is-primary"
			on:click=move |_| set_open.set(true)
		>
			"Import Ticket"
		</button>

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
						{move || parsed_ticket.get().map(|t| view! {
							<table class="table is-fullwidth is-narrow mb-4">
								<tbody>
									<tr>
										<th>"Server ID"</th>
										<td class="is-family-monospace">{t.server_id.to_string()}</td>
									</tr>
									<tr>
										<th>"Host"</th>
										<td class="is-family-monospace">{t.canonical_url.clone()}</td>
									</tr>
									<tr>
										<th>"Hostname"</th>
										<td>{t.hostname.clone()}</td>
									</tr>
									{t.tailscale_ip.map(|ip| view! {
										<tr>
											<th>"Tailscale IP"</th>
											<td class="is-family-monospace">{ip}</td>
										</tr>
									})}
									{t.hosting.map(|h| view! {
										<tr>
											<th>"Hosting"</th>
											<td>{h}</td>
										</tr>
									})}
								</tbody>
							</table>
						})}
						<div class="field is-grouped">
							<div class="field mr-4">
								<label class="label">"Kind"</label>
								<div class="control">
									<div class="select">
										<select
											prop:value=move || kind.get().to_string()
											disabled=move || kind_from_ticket.get().is_some()
											on:change=move |ev| set_kind.set(
												event_target_value(&ev).parse().unwrap_or_default()
											)
										>
											<option value={ServerKind::Facility}>{ServerKind::Facility}</option>
											<option value={ServerKind::Central}>{ServerKind::Central}</option>
										</select>
									</div>
								</div>
							</div>
							<div class="field">
								<label class="label">"Rank"</label>
								<div class="control">
									<div class="select">
										<select
											prop:value=move || rank.get().map_or_else(String::new, |r| r.to_string())
											disabled=move || rank_from_ticket.get().is_some()
											on:change=move |ev| set_rank.set(event_target_value(&ev).parse().ok())
										>
											<option value="">"unranked"</option>
											<option value={ServerRank::Production}>{ServerRank::Production}</option>
											<option value={ServerRank::Clone}>{ServerRank::Clone}</option>
											<option value={ServerRank::Demo}>{ServerRank::Demo}</option>
											<option value={ServerRank::Test}>{ServerRank::Test}</option>
											<option value={ServerRank::Dev}>{ServerRank::Dev}</option>
										</select>
									</div>
								</div>
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
