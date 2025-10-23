use leptos::prelude::*;

#[derive(Clone, Debug)]
pub struct ToastCtx(pub WriteSignal<Option<String>>);

#[component]
pub fn Toast(children: Children) -> impl IntoView {
	let (message, set_message) = signal(None::<String>);

	provide_context(ToastCtx(set_message));

	view! {
		<dialog open={move || message.get().is_some()}>
			<p>{message}</p>
		</dialog>
		{children()}
	}
}
