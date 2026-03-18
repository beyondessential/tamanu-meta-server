use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

#[component]
pub fn ImportTicket() -> impl IntoView {
	let (ticket, set_ticket) = signal(String::new());
	let (error, set_error) = signal(Option
