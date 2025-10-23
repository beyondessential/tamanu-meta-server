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
				format_secs((diff_ms / 1000.0).abs() as _)
			} else {
				"?".to_string()
			}
		};

		// Update immediately
		set_ago_text.set(format_duration());

		// Set up interval to update every 10 seconds while page is visible
		Effect::new(move |_| {
			let _ = leptos::prelude::set_interval(
				move || {
					if let Some(document) = web_sys::window().and_then(|w| w.document()) {
						if !document.hidden() {
							set_ago_text.set(format_duration());
						}
					}
				},
				std::time::Duration::from_secs(10),
			);
		});
	}

	#[cfg(feature = "ssr")]
	{
		use std::str::FromStr as _;
		match jiff::Timestamp::from_str(&timestamp) {
			Ok(parsed) => {
				let now = jiff::Timestamp::now();
				let diff = now.duration_since(parsed);
				let secs = diff.as_secs().abs() as u64;
				set_ago_text.set(format_secs(secs));
			}
			Err(err) => {
				set_ago_text.set(err.to_string());
			}
		}
	}

	view! {
		<span class="time-ago" title={timestamp.clone()}>
		{move || ago_text.get()} " ago"
		</span>
	}
}

fn format_secs(secs: u64) -> String {
	if secs < 3600 {
		let minutes = secs / 60;
		format!("{}m", minutes)
	} else if secs < 86400 {
		let hours = secs / 3600;
		format!("{}h", hours)
	} else {
		let days = secs / 86400;
		format!("{}d", days)
	}
}
