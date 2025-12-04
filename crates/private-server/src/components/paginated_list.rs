use leptos::prelude::*;

#[component]
pub fn PaginatedList(
	/// Current page (0-indexed)
	page: ReadSignal<u64>,

	/// Function to update page
	set_page: WriteSignal<u64>,

	/// Total count of items
	total_count: Signal<u64>,

	/// Number of items per page
	#[prop(default = 10)]
	page_size: u64,

	/// Content to render
	children: Children,
) -> impl IntoView {
	let last_page = Signal::derive({
		let count = total_count;
		move || count.get().saturating_div(page_size)
	});

	view! {
		<nav class="pagination" role="navigation" aria-label="pagination">
			<button
				class="pagination-previous"
				class:is-disabled=move || page.get() == 0
				disabled=move || page.get() == 0
				on:click=move |_| set_page.update(|p| *p = (*p).saturating_sub(1))
			>"Previous"</button>
			<button
				class="pagination-next"
				class:is-disabled=move || page.get() == last_page.get()
				disabled=move || page.get() == last_page.get()
				on:click=move |_| set_page.update(|p| *p = (*p).saturating_add(1))
			>"Next page"</button>
			<ul class="pagination-list">{move || {
				if last_page.get() < 7 {
					view! { <ShowAll page set_page last_page /> }.into_any()
				}
				else if page.get() < 2 {
					view! { <AtStart page set_page last_page /> }.into_any()
				}
				else if page.get() > last_page.get().saturating_sub(2) {
					view! { <AtEnd page set_page last_page /> }.into_any()
				}
				else {
					view! { <InMiddle page set_page last_page /> }.into_any()
				}
			}}</ul>
		</nav>
		{children()}
	}
}

// ShowAll  = 1234567 = last_page < 7
// AtStart  = 123...8 = page < 2
// AtEnd    = 1...678 = page > last_page - 2
// InMiddle = 1.456.9 = else

#[component]
fn ShowAll(
	page: ReadSignal<u64>,
	set_page: WriteSignal<u64>,
	last_page: Signal<u64>,
) -> impl IntoView {
	view! {
		<For each=move || 0..=last_page.get() key=|n| *n let:n>
			<li><button
				class="pagination-link"
				class:is-current={move || n == page.get()}
				attr:aria-current:page={move || n == page.get()}
				aria-label={move || format!("Goto page {}", n + 1)}
				on:click=move |_| set_page.update(|p| *p = n)
			>{n + 1}</button></li>
		</For>
	}
}

#[component]
fn AtStart(
	page: ReadSignal<u64>,
	set_page: WriteSignal<u64>,
	last_page: Signal<u64>,
) -> impl IntoView {
	view! {
		<For each=move || 0..=2 key=|n| *n let:n>
			<li><button
				class="pagination-link"
				class:is-current={move || n == page.get()}
				attr:aria-current:page={move || n == page.get()}
				aria-label={move || format!("Goto page {}", n + 1)}
				on:click=move |_| set_page.update(|p| *p = n)
			>{n + 1}</button></li>
		</For>
		<li><span class="pagination-ellipsis">"…"</span></li>
		<li><button
			class="pagination-link"
			class:is-current={move || last_page.get() == page.get()}
			attr:aria-current:page={move || last_page.get() == page.get()}
			aria-label={move || format!("Goto page {}", last_page.get() + 1)}
			on:click=move |_| set_page.update(|p| *p = last_page.get())
		>{move || last_page.get() + 1}</button></li>
	}
}

#[component]
fn AtEnd(
	page: ReadSignal<u64>,
	set_page: WriteSignal<u64>,
	last_page: Signal<u64>,
) -> impl IntoView {
	view! {
		<li><button
			class="pagination-link"
			class:is-current={move || page.get() == 0}
			attr:aria-current:page={move || page.get() == 0}
			aria-label="Goto page 1"
			on:click=move |_| set_page.update(|p| *p = 0)
		>"1"</button></li>
		<li><span class="pagination-ellipsis">"…"</span></li>
		<For each=move || (last_page.get() - 2)..=last_page.get() key=|n| *n let:n>
			<li><button
				class="pagination-link"
				class:is-current={move || n == page.get()}
				attr:aria-current:page={move || n == page.get()}
				aria-label={move || format!("Goto page {}", n + 1)}
				on:click=move |_| set_page.update(|p| *p = n)
			>{n + 1}</button></li>
		</For>
	}
}

#[component]
fn InMiddle(
	page: ReadSignal<u64>,
	set_page: WriteSignal<u64>,
	last_page: Signal<u64>,
) -> impl IntoView {
	view! {
		<li><button
			class="pagination-link"
			class:is-current={move || page.get() == 0}
			attr:aria-current:page={move || page.get() == 0}
			aria-label="Goto page 1"
			on:click=move |_| set_page.update(|p| *p = 0)
		>"1"</button></li>
		{move || (page.get() != 2).then(|| view! {
			<li><span class="pagination-ellipsis">"…"</span></li>
		})}
		<For each=move || (page.get() - 1)..=(page.get() + 1) key=|n| *n let:n>
			<li><button
				class="pagination-link"
				class:is-current={move || n == page.get()}
				attr:aria-current:page={move || n == page.get()}
				aria-label={move || format!("Goto page {}", n + 1)}
				on:click=move |_| set_page.update(|p| *p = n)
			>{n + 1}</button></li>
		</For>
		{move || (page.get() != (last_page.get() - 2)).then(|| view! {
			<li><span class="pagination-ellipsis">"…"</span></li>
		})}
		<li><button
			class="pagination-link"
			class:is-current={move || last_page.get() == page.get()}
			attr:aria-current:page={move || last_page.get() == page.get()}
			aria-label={move || format!("Goto page {}", last_page.get() + 1)}
			on:click=move |_| set_page.update(|p| *p = last_page.get())
		>{move || last_page.get() + 1}</button></li>
	}
}
