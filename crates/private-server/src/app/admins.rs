use leptos::prelude::*;
use leptos_meta::{Stylesheet, provide_meta_context};

use crate::components::ToastCtx;

#[component]
pub fn Page() -> impl IntoView {
	provide_meta_context();
	let list = LocalResource::new(async || crate::fns::admins::list().await);

	view! {
		<Stylesheet id="css-admin" href="/static/admin.css" />
		<section class="section">
			<div class="columns">
				<div class="column">
					<AddAdmin after_add=move || list.refetch() />
				</div>
				<div class="column">
					<Suspense fallback=|| view! { <progress class="progress is-small is-primary" max="100">"Loading..."</progress> }>
						{move || list.get().map(|data| match data {
							Ok(admins) => {
								if admins.is_empty() {
									view! {
										<div class="has-info-text">"No admins configured"</div>
									}.into_any()
								} else {
									view! {
										<ListAdmins admins after_del=move || list.refetch() />
									}.into_any()
								}
							}
							Err(err) => {
								view! {
									<div class="has-danger-text">{format!("Error loading admins: {err}")}</div>
								}.into_any()
							}
						})}
					</Suspense>
				</div>
			</div>
		</section>
	}
}

#[component]
fn AddAdmin(after_add: impl Fn() + Send + Copy + 'static) -> impl IntoView {
	let ToastCtx(set_message) = use_context().unwrap();
	let (email, set_email) = signal(String::new());

	let add_admin = Action::new(move |email: &String| {
		let email = email.clone();
		async move { crate::fns::admins::add(email).await }
	});

	let on_submit = move |ev: web_sys::SubmitEvent| {
		ev.prevent_default();
		let email_value = email.get().trim().to_string();

		if email_value.is_empty() {
			set_message.set(Some("Email cannot be empty".to_string()));
			return;
		}

		if !email_value.contains('@') {
			set_message.set(Some("Please enter a valid email address".to_string()));
			return;
		}

		add_admin.dispatch(email_value);
		set_email.set(String::new());
	};

	Effect::new(move |_| {
		if let Some(result) = add_admin.value().get() {
			match result {
				Ok(_) => {
					set_message.set(Some("Admin added successfully".to_string()));
					after_add();

					set_timeout(
						move || set_message.set(None),
						std::time::Duration::from_millis(3000),
					);
				}
				Err(e) => {
					set_message.set(Some(format!("Error adding admin: {}", e)));
				}
			}
		}
	});

	view! {
		<div class="box">
			<form on:submit=on_submit>
				<div class="field has-addons">
					<div class="control is-expanded">
						<input
							type="email"
							class="input"
							name="email"
							prop:value=move || email.get()
							on:input=move |ev| set_email.set(event_target_value(&ev))
							placeholder="admin@example.com"
						/>
					</div>
					<div class="control">
						<button
							type="submit"
							class="button is-primary"
							disabled=move || add_admin.pending().get()
						>
							{move || if add_admin.pending().get() { "Adding..." } else { "Add Admin" }}
						</button>
					</div>
				</div>
			</form>
		</div>
	}
}

#[component]
fn ListAdmins(admins: Vec<String>, after_del: impl Fn() + Send + Copy + 'static) -> impl IntoView {
	let ToastCtx(set_message) = use_context().unwrap();

	let delete_admin = Action::new(move |email: &String| {
		let email = email.clone();
		async move { crate::fns::admins::delete(email).await }
	});

	Effect::new(move |_| {
		if let Some(result) = delete_admin.value().get() {
			match result {
				Ok(_) => {
					after_del();
				}
				Err(e) => {
					set_message.set(Some(format!("Error deleting admin: {}", e)));
				}
			}
		}
	});

	view! {
		<For each=move || admins.clone() key=|a| a.clone() let:admin>
			<div class="box level">
				<div class="level-left">
					<span class="level-item monospace">{admin.clone()}</span>
				</div>
				<div class="level-right">
					<button
						class="level-item button is-danger"
						on:click=move |_| drop(delete_admin.dispatch(admin.clone()))
						disabled=move || delete_admin.pending().get()
					>
						{move || if delete_admin.pending().get() { "Deleting..." } else { "Delete" }}
					</button>
				</div>
			</div>
		</For>
	}
}
