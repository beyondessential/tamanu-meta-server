use std::sync::Arc;

use commons_errors::AppError;
use leptos::prelude::*;

use crate::{
	components::{DeviceShorty, PaginatedList},
	fns::devices::DeviceInfo,
};

const PAGE_SIZE: u64 = 10;

#[component]
pub fn Trusted() -> impl IntoView {
	let count = Resource::new(
		|| (),
		async |_| {
			crate::fns::devices::count_trusted()
				.await
				.unwrap_or_default()
		},
	);

	let fetcher = |p| async move {
		let offset = p * PAGE_SIZE;
		crate::fns::devices::list_trusted(Some(PAGE_SIZE), Some(offset)).await
	};

	view! {
		<List count fetcher />
	}
}

#[component]
pub fn Untrusted() -> impl IntoView {
	let count = Resource::new(
		|| (),
		async |_| {
			crate::fns::devices::count_untrusted()
				.await
				.unwrap_or_default()
		},
	);

	let fetcher = |p| async move {
		let offset = p * PAGE_SIZE;
		crate::fns::devices::list_untrusted(Some(PAGE_SIZE), Some(offset)).await
	};

	view! {
		<List count fetcher />
	}
}

#[component]
fn List<F, T>(count: Resource<u64>, fetcher: F) -> impl IntoView
where
	F: Fn(u64) -> T + Send + Sync + 'static,
	T: Future<Output = Result<Vec<Arc<DeviceInfo>>, AppError>> + Send + 'static,
{
	let (page, set_page) = signal(0u64);
	let devices = Resource::new(move || page.get(), fetcher);

	view! {
		<section class="section">
			<Transition fallback=|| view! { <progress class="progress is-small is-primary" max="100">"Loading..."</progress> }>
				{move || devices.get().map(|result| {
					match result {
						Ok(devices) => {
							if devices.is_empty() {
								view! {
									<div class="box has-text-info">"No devices found"</div>
								}.into_any()
							} else {
								view! {
									<PaginatedList
										page=page
										set_page=set_page
										total_count=Signal::derive(move || count.get().unwrap_or(0))
										page_size=PAGE_SIZE
									>
										<DeviceList devices=devices.clone() />
									</PaginatedList>
								}.into_any()
							}
						}
						Err(e) => {
							view! {
								<div class="has-text-danger">{format!("Error loading devices: {}", e)}</div>
							}.into_any()
						}
					}
				})}
			</Transition>
		</section>
	}
}

#[component]
pub fn DeviceList(devices: Vec<Arc<crate::fns::devices::DeviceInfo>>) -> impl IntoView {
	view! {
		<For each=move || devices.clone() key=|device| device.device.id let:device>
			<DeviceShorty device=device.clone() {..} class="level box" />
		</For>
	}
}
