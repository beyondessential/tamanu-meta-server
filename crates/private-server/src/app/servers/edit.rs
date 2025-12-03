use commons_errors::AppError;
use commons_types::server::{kind::ServerKind, rank::ServerRank};
use leptos::prelude::*;
use leptos_router::{components::A, hooks::use_params_map};

use crate::{
	components::{ErrorHandler, LoadingBar},
	fns::servers::{ServerDataUpdate, ServerInfo, get_info, update},
};

#[component]
pub fn Edit() -> impl IntoView {
	let params = use_params_map();
	let server_id = move || {
		params
			.read()
			.get("id")
			.map(|id| id.parse().ok())
			.flatten()
			.unwrap_or_default()
	};

	let data = Resource::new(move || server_id(), async move |id| get_info(id).await);

	view! {
		<Transition fallback=|| { view!{ <LoadingBar /> } }>
			<ErrorHandler>
				{move || data.and_then(|info| {
					let info = info.clone();
					view! { <EditForm info /> }
				})}
			</ErrorHandler>
		</Transition>
	}
}

#[component]
pub fn EditForm(info: ServerInfo) -> impl IntoView {
	let server_id = info.id;
	let (name, set_name) = signal(info.name.clone());
	let (host, set_host) = signal(info.host.clone());
	let (kind, set_kind) = signal(info.kind);
	let (rank, set_rank) = signal(info.rank);
	let (listed, set_listed) = signal(info.listed);

	let save_data = Signal::derive(move || ServerDataUpdate {
		name: name.get(),
		host: Some(host.get()),
		kind: Some(kind.get()),
		rank: rank.get(),
		listed: Some(listed.get()),
		..Default::default()
	});

	let navigate = leptos_router::hooks::use_navigate();
	let submit = Action::new(move |sig: &Signal<ServerDataUpdate>| {
		let data = sig.get();
		let navigate = navigate.clone();
		async move {
			update(server_id, data).await?;
			navigate(&format!("/servers/{server_id}"), Default::default());
			Ok::<_, AppError>(())
		}
	});

	view! {
		<form class="box" on:submit=move |ev| {
			ev.prevent_default();
			submit.dispatch(save_data);
		}>
			<div class="field is-horizontal">
				<div class="field-label is-normal">
					<label class="label" for="field-name">Name</label>
				</div>
				<div class="field-body">
					<div class="field">
						<div class="control">
							<input
								id="field-name"
								name="name"
								class="input"
								type="text"
								disabled=move || submit.pending().get()
								prop:value=move || name.get()
								on:change=move |ev| set_name.set(none_if_empty(event_target_value(&ev))) />
						</div>
					</div>
				</div>
			</div>
			<div class="field is-horizontal">
				<div class="field-label is-normal">
					<label class="label" for="field-host">URL</label>
				</div>
				<div class="field-body">
					<div class="field">
						<div class="control">
							<input
								id="field-host"
								name="host"
								class="input"
								type="text"
								disabled=move || submit.pending().get()
								prop:value=move || host.get()
								on:change=move |ev| set_host.set(event_target_value(&ev)) />
						</div>
					</div>
				</div>
			</div>
			<div class="field is-horizontal">
				<div class="field-label is-normal">
					<label class="label" for="field-kind">Kind</label>
				</div>
				<div class="field-body">
					<div class="field">
						<div class="control">
							<div class="select">
								<select
									id="field-kind"
									disabled=move || submit.pending().get()
									prop:value=move || kind.get()
									on:change=move |ev| set_kind.set(event_target_value(&ev).parse().unwrap_or_default())
								>
									<option value={ServerKind::Central}>{ServerKind::Central}</option>
									<option value={ServerKind::Facility}>{ServerKind::Facility}</option>
								</select>
							</div>
						</div>
					</div>
				</div>
			</div>
			<div class="field is-horizontal">
				<div class="field-label is-normal">
					<label class="label" for="field-rank">Rank</label>
				</div>
				<div class="field-body">
					<div class="field">
						<div class="control">
							<div class="select">
								<select
									id="field-rank"
									disabled=move || submit.pending().get()
									prop:value=move || rank.get().map_or("".to_string(), |rank| rank.to_string())
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
			</div>
			<div class="field is-horizontal">
				<div class="field-label"></div>
				<div class="field-body">
					<div class="field">
						<div class="control">
							<label class="checkbox">
								<input
									class="mr-2"
									type="checkbox"
									disabled=move || submit.pending().get()
									prop:checked=move || listed.get()
									on:change=move |ev| set_listed.set(event_target_checked(&ev)) />
								"Available in Tamanu Mobile app"
							</label>
						</div>
					</div>
				</div>
			</div>
			<div class="field is-horizontal">
				<div class="field-label"></div>
				<div class="field-body">
					<div class="field is-grouped">
						<div class="control">
							<button
								type="submit"
								class="button is-primary"
								disabled=move || submit.pending().get()
							>Save</button>
						</div>
						<div class="control">
							<A
								href=format!("/servers/{server_id}")
								{..}
								class="button is-danger is-light"
								class:is-disabled=move || submit.pending().get()
							>Cancel</A>
						</div>
					</div>
				</div>
			</div>
		</form>
	}
}

fn none_if_empty(s: String) -> Option<String> {
	if s.is_empty() { None } else { Some(s) }
}
