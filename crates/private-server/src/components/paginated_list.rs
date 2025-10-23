use leptos::prelude::*;

#[component]
pub fn PaginatedList(
	/// Current page (0-indexed)
	page: ReadSignal<i64>,
	/// Function to update page
	set_page: WriteSignal<i64>,
	/// Total count of items
	total_count: Signal<Option<i64>>,
	/// Number of items per page
	#[prop(default = 10)]
	page_size: i64,
	/// Content to render
	children: Children,
) -> impl IntoView {
	view! {
		<div class="paginated-list-container">
			{children()}
			<div class="pagination">
				<button
					class="pagination-btn"
					on:click=move |_| set_page.update(|p| *p = (*p).saturating_sub(1))
					disabled=move || page.get() == 0
				>
					"← Previous"
				</button>
				<span class="pagination-info">
					{move || {
						total_count
							.get()
							.map(|total| {
								let current_page = page.get() + 1;
								let total_pages = ((total as f64) / (page_size as f64)).ceil() as i64;
								format!("Page {} of {}", current_page, total_pages.max(1))
							})
							.unwrap_or_else(|| "Loading...".to_string())
					}}
				</span>
				<button
					class="pagination-btn"
					on:click=move |_| set_page.update(|p| *p += 1)
					disabled=move || {
						total_count
							.get()
							.map(|total| (page.get() + 1) * page_size >= total)
							.unwrap_or(true)
					}
				>
					"Next →"
				</button>
			</div>
		</div>
	}
}
