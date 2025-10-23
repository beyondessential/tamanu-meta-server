use leptos::prelude::*;

#[component]
pub fn TimeAgo(timestamp: String) -> impl IntoView {
	let (ago_text, set_ago_text) = signal(String::new());

	#[cfg(not(feature = "ssr"))]
	{
		let parsed_timestamp_ms = {
			if let Some(_window) = web_sys::window() {
				let js_date = web_sys::js_sys::Date::new(&timestamp.clone().into());
				let time_value = js_date.get_time();
				if !time_value.is_nan() {
					Some(time_value)
				} else {
					None
				}
			} else {
				None
			}
		};

		let format_duration = move || -> String {
			if let Some(timestamp_ms) = parsed_timestamp_ms {
				let now_ms = web_sys::js_sys::Date::now();
				let diff_ms = now_ms - timestamp_ms;
				let total_seconds = (diff_ms / 1000.0).abs() as i64;

				if total_seconds < 3600 {
					let minutes = total_seconds / 60;
					format!("{}m", minutes)
				} else if total_seconds < 86400 {
					let hours = total_seconds / 3600;
					format!("{}h", hours)
				} else {
					let days = total_seconds / 86400;
					format!("{}d", days)
				}
			} else {
				"unknown".to_string()
			}
		};

		// Update immediately
		set_ago_text.set(format_duration());

		// Set up interval to update every second while page is visible
		Effect::new(move |_| {
			let _ = leptos::prelude::set_interval(
				move || {
					if let Some(document) = web_sys::window().and_then(|w| w.document()) {
						if !document.hidden() {
							set_ago_text.set(format_duration());
						}
					}
				},
				std::time::Duration::from_secs(1),
			);
		});
	}

	#[cfg(feature = "ssr")]
	{
		// On SSR, just show a placeholder
		let _ = timestamp; // Suppress unused variable warning
		set_ago_text.set("...".to_string());
	}

	view! {
		{move || ago_text.get()}
	}
}
