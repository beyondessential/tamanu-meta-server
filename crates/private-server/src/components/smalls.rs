use std::fmt::Display;

use commons_types::{
	device::DeviceRole,
	server::{kind::ServerKind, rank::ServerRank},
	status::ShortStatus,
	version::VersionStatus,
};
use leptos::prelude::*;

#[component]
pub fn LoadingBar() -> impl IntoView {
	view! {
		<progress class="progress is-small is-primary" max="100">"Loading…"</progress>
	}
}

#[component]
pub fn Error(error: impl Display) -> impl IntoView {
	view! {
		<div class="has-text-danger">{error.to_string()}</div>
	}
}

#[component]
pub fn ErrorHandler<Chil>(children: TypedChildren<Chil>) -> impl IntoView
where
	Chil: IntoView + Send + 'static,
{
	view! {
		<ErrorBoundary fallback=move |errors| { view! {
			<Transition fallback=|| view! { <Error error="Unknown error" /> }>
				{move || {
					let errors = errors.get();
					view! {
						<For each=move || errors.clone() key=|(id, _)| id.clone() let:((_, error))>
							<Error error={error} {..} class:box />
						</For>
					}
				}}
			</Transition>
		} }>
			{children.into_inner()()}
		</ErrorBoundary>
	}
}

#[component]
pub fn Nothing(#[prop(optional)] thing: Option<impl Display>) -> impl IntoView {
	view! {
		<div class="box has-text-info">{thing.map(|t| format!("No {t} found")).unwrap_or("Nothing found".to_string())}</div>
	}
}

#[component]
pub fn DeviceRoleBadge(role: DeviceRole) -> impl IntoView {
	view! {
		<span class={format!("level-item tag is-capitalized {}", match role {
			DeviceRole::Untrusted => "is-danger",
			DeviceRole::Server => "is-primary",
			DeviceRole::Releaser => "is-warning",
			DeviceRole::Admin => "is-info",
		})}>{role}</span>
	}
}

#[component]
pub fn ServerKindBadge(kind: ServerKind) -> impl IntoView {
	view! {
		<span class={format!("level-item tag is-capitalized {}", match kind {
			ServerKind::Central => "is-link",
			ServerKind::Facility => "is-info",
			ServerKind::Meta => ""
		})}>{kind}</span>
	}
}

#[component]
pub fn ServerRankBadge(rank: ServerRank) -> impl IntoView {
	view! {
		<span class={format!("level-item tag is-capitalized {}", match rank {
			ServerRank::Production => "is-danger",
			ServerRank::Clone => "is-warning",
			ServerRank::Demo => "is-link",
			ServerRank::Test => "is-info",
			ServerRank::Dev => "is-success",
		})}>{rank}</span>
	}
}

#[component]
pub fn StatusDot(
	up: ShortStatus,
	#[prop(optional)] name: Option<String>,
	#[prop(optional)] kind: ServerKind,
) -> impl IntoView {
	view! {
		<span
			class={format!("status-dot {}", up)}
			class:facility-dot={kind != ServerKind::Central}
			title={name.map(|name| format!("{}: {}", name, up))}
		></span>
	}
}

#[component]
pub fn VersionStatusBadge(status: VersionStatus) -> impl IntoView {
	view! {
		<span class={format!("level-item tag is-capitalized {}", match status {
			VersionStatus::Draft => "is-warning",
			VersionStatus::Published => "is-success",
			VersionStatus::Yanked => "is-danger",
		})}>{status}</span>
	}
}

pub trait ToggleSignal {
	fn toggle(&self);
}

impl ToggleSignal for WriteSignal<bool> {
	fn toggle(&self) {
		self.update(|it| *it = !*it);
	}
}
