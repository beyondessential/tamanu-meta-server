use leptos::prelude::*;
use leptos_meta::{Stylesheet, provide_meta_context};

#[component]
pub fn Page() -> impl IntoView {
	provide_meta_context();
	let is_admin = Resource::new(
		|| (),
		|_| async { crate::fns::commons::is_current_user_admin().await },
	);

	view! {
		<Stylesheet id="css-admin" href="/static/admin.css" />
		<section class="section" id="admin-page">
		<Suspense fallback=|| view! { <div class="loading">"Checking permissions..."</div> }>
			{move || {
				is_admin.get().map(|result| {
					match result {
						Ok(true) => {
							view! {
								<AdminManagement />
							}.into_any()
						}
						Ok(false) => {
							view! {
								<div class="page-header">
									<h1>"Access Denied"</h1>
								</div>
								<div class="error">
									<p>"You do not have permission to access the admin panel."</p>
									<a href="/" class="back-link">"← Return to Home"</a>
								</div>
							}.into_any()
						}
						Err(e) => {
							view! {
								<div class="page-header">
									<h1>"Error"</h1>
								</div>
								<div class="error">
									{format!("Error checking permissions: {}", e)}
								</div>
							}.into_any()
						}
					}
				})
			}}
		</Suspense>
		</section>
	}
}

#[component]
pub fn AdminManagement() -> impl IntoView {
	let (email, set_email) = signal(String::new());
	let (message, set_message) = signal(String::new());
	let (refresh_trigger, set_refresh_trigger) = signal(0);

	let list = Resource::new(
		move || refresh_trigger.get(),
		async |_| crate::fns::admins::list().await,
	);

	let add_admin = Action::new(move |email: &String| {
		let email = email.clone();
		async move { crate::fns::admins::add(email).await }
	});

	let delete_admin = Action::new(move |email: &String| {
		let email = email.clone();
		async move { crate::fns::admins::delete(email).await }
	});

	let on_submit = move |ev: web_sys::SubmitEvent| {
		ev.prevent_default();
		let email_value = email.get().trim().to_string();

		if email_value.is_empty() {
			set_message.set("Email cannot be empty".to_string());
			return;
		}

		if !email_value.contains('@') {
			set_message.set("Please enter a valid email address".to_string());
			return;
		}

		add_admin.dispatch(email_value);
		set_email.set(String::new());
	};

	Effect::new(move |_| {
		if let Some(result) = add_admin.value().get() {
			match result {
				Ok(_) => {
					set_message.set("Admin added successfully".to_string());
					set_refresh_trigger.update(|n| *n += 1);

					set_timeout(
						move || set_message.set(String::new()),
						std::time::Duration::from_millis(3000),
					);
				}
				Err(e) => {
					set_message.set(format!("Error adding admin: {}", e));
				}
			}
		}
	});

	Effect::new(move |_| {
		if let Some(result) = delete_admin.value().get() {
			match result {
				Ok(_) => {
					set_refresh_trigger.update(|n| *n += 1);
				}
				Err(e) => {
					set_message.set(format!("Error deleting admin: {}", e));
				}
			}
		}
	});

	view! {
		<div class="add-admin-form">
			<h2>"Add New Admin"</h2>
			<form on:submit=on_submit>
				<div>
					<label for="email">"Email:"</label>
					<input
						type="email"
						id="email"
						prop:value=move || email.get()
						on:input=move |ev| set_email.set(event_target_value(&ev))
						placeholder="admin@example.com"
					/>
				</div>
				<button type="submit" disabled=move || add_admin.pending().get()>
					{move || if add_admin.pending().get() { "Adding..." } else { "Add Admin" }}
				</button>
			</form>
			{move || {
				let msg = message.get();
				if !msg.is_empty() {
					view! { <div class="message">{msg}</div> }.into_any()
				} else {
					().into_any()
				}
			}}
		</div>

		<div class="admin-list">
			<h2>"Current Admins"</h2>
			<Suspense fallback=|| view! { <div class="loading">"Loading…"</div> }>
				{move || list.get().map(|data| match data {
					Ok(admins) => {
						if admins.is_empty() {
							view! {
								<div class="no-admins">"No admins configured"</div>
							}.into_any()
						} else {
							view! {
								<ul class="admin-items">
									<For each=move || admins.clone() key=|a| a.clone() let:admin>
										<li class="admin-item">
											<span class="admin-email monospace">{admin.clone()}</span>
											<button
												class="delete-btn"
												on:click=move |_| {
													let _ = delete_admin.dispatch(admin.clone());
												}
												disabled=move || delete_admin.pending().get()
											>
												{move || if delete_admin.pending().get() { "Deleting..." } else { "Delete" }}
											</button>
										</li>
									</For>
								</ul>
							}.into_any()
						}
					}
					Err(e) => {
						view! {
							<div class="error">{format!("Error loading admins: {}", e)}</div>
						}.into_any()
					}
				})}
			</Suspense>
		</div>
	}
}
