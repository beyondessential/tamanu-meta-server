use std::sync::Arc;

use commons_errors::AppError;
use commons_types::server::kind::ServerKind;
use leptos::prelude::*;

use crate::{
	components::{Error, LoadingBar, Nothing, PaginatedList, ServerShorty},
	fns::servers::{ServerInfo, count_servers, list_servers},
};

const PAGE_SIZE: u64 = 10;

#[component]
pub fn Centrals() -> impl IntoView {
	view! {
		<ListServers kind=ServerKind::Central />
	}
}

#[component]
pub fn Facilities() -> impl IntoView {
	view! {
		<ListServers kind=ServerKind::Facility />
	}
}

#[component]
fn ListServers(kind: ServerKind) -> impl IntoView {
	let count = Resource::new(
		|| (),
		move |_| async move { count_servers(Some(kind)).await.unwrap_or_default() },
	);

	let fetcher = move |p| async move {
		let offset = p * PAGE_SIZE;
		list_servers(Some(kind), offset, Some(PAGE_SIZE)).await
	};

	view! {
		<List count fetcher />
	}
}

#[component]
fn List<F, T>(count: Resource<u64>, fetcher: F) -> impl IntoView
where
	F: Fn(u64) -> T + Send + Sync + 'static,
	T: Future<Output = Result<Vec<Arc<ServerInfo>>, AppError>> + Send + 'static,
{
	let (page, set_page) = signal(0u64);
	let servers = Resource::new(move || page.get(), fetcher);

	view! {
		<section class="section">
			<Transition fallback=|| view! { <LoadingBar /> }>
				{move || servers.get().map(|result| {
					match result {
						Ok(servers) => {
							if servers.is_empty() {
								view! {
									<Nothing thing="servers" />
								}.into_any()
							} else {
								view! {
									<PaginatedList
										page=page
										set_page=set_page
										total_count=Signal::derive(move || count.get().unwrap_or(0))
										page_size=PAGE_SIZE
									>
										<ServerList servers=servers.clone() />
									</PaginatedList>
								}.into_any()
							}
						}
						Err(error) => {
							view! {
								<Error context="Error loading servers" error />
							}.into_any()
						}
					}
				})}
			</Transition>
		</section>
	}
}

#[component]
pub fn ServerList(servers: Vec<Arc<ServerInfo>>) -> impl IntoView {
	view! {
		<For each=move || servers.clone() key=|server| server.id let:server>
			<ServerShorty server=server.clone() {..} class="level box" />
		</For>
	}
}
