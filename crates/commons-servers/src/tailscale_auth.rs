use axum::extract::{FromRef, FromRequestParts, OptionalFromRequestParts};
use commons_errors::AppError;
use database::{Db, admins::Admin, tailscale_users::TailscaleUser as CachedTailscaleUser};
use diesel_async::AsyncPgConnection;
use http::request::Parts;

use crate::tailnet_directory::TailnetDirectory;

const TAILSCALE_USER_LOGIN: &str = "Tailscale-User-Login";
const TAILSCALE_USER_NAME: &str = "Tailscale-User-Name";
const TAILSCALE_USER_PROFILE_PIC: &str = "Tailscale-User-Profile-Pic";

/// Dev/test-only header that downgrades the auth bypass to a non-admin
/// identity, so tests can exercise the read-only, non-admin UI against a
/// debug build. Honoured under the same `debug_assertions` gate as the
/// bypass itself, and it only ever *drops* privileges — the branch that
/// reads it is not compiled into release builds, so it carries no risk
/// there.
const DEV_NON_ADMIN_HEADER: &str = "x-canopy-dev-non-admin";

#[derive(Debug, Clone, Default)]
pub struct TailscaleUser {
	pub login: String,
	pub name: String,
	pub profile_pic: Option<String>,
}

impl TailscaleUser {
	/// Admin if the login is on the recorded allowlist, or the tailnet policy
	/// grants it admin (see [`crate::tailnet_directory`]). The allowlist is
	/// checked first, so an unreachable control plane can't lock out an
	/// allowlisted admin.
	pub async fn is_admin(
		&self,
		db: &mut AsyncPgConnection,
		directory: Option<&TailnetDirectory>,
	) -> Result<bool, AppError> {
		if Admin::check_email(db, &self.login).await? {
			return Ok(true);
		}
		if let Some(directory) = directory
			&& directory.is_admin_by_policy(&self.login).await
		{
			return Ok(true);
		}
		Ok(false)
	}
}

impl<S> FromRequestParts<S> for TailscaleUser
where
	S: Send + Sync,
{
	type Rejection = AppError;

	async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
		// Dev / test bypass: mirrors `TailscaleAdmin`. Without it, every
		// integration test would need to set the three Tailscale headers on
		// every authenticated request, even for endpoints that only need
		// "any user". A non-admin and an admin are both "any user", so the
		// non-admin override only changes the identity, not the outcome here.
		if cfg!(debug_assertions) {
			let login = if parts.headers.contains_key(DEV_NON_ADMIN_HEADER) {
				"user@localhost"
			} else {
				"admin@localhost"
			};
			return Ok(TailscaleUser {
				login: login.into(),
				name: "You".into(),
				profile_pic: None,
			});
		}
		let login = parts
			.headers
			.get(TAILSCALE_USER_LOGIN)
			.ok_or(AppError::AuthMissingHeader(TAILSCALE_USER_LOGIN))
			.and_then(|value| {
				rfc2047_decoder::decode(value.as_bytes()).or_else(|_| {
					value
						.to_str()
						.map_err(|err| AppError::custom(format!("invalid header format: {err}")))
						.map(ToOwned::to_owned)
				})
			})?;
		let name = parts
			.headers
			.get(TAILSCALE_USER_NAME)
			.ok_or(AppError::AuthMissingHeader(TAILSCALE_USER_NAME))
			.and_then(|value| {
				rfc2047_decoder::decode(value.as_bytes()).or_else(|_| {
					value
						.to_str()
						.map_err(|err| AppError::custom(format!("invalid header format: {err}")))
						.map(ToOwned::to_owned)
				})
			})?;
		let profile_pic = parts
			.headers
			.get(TAILSCALE_USER_PROFILE_PIC)
			.map(|value| {
				rfc2047_decoder::decode(value.as_bytes()).or_else(|_| {
					value
						.to_str()
						.map_err(|err| AppError::custom(format!("invalid header format: {err}")))
						.map(ToOwned::to_owned)
				})
			})
			.transpose()?;

		Ok(TailscaleUser {
			login,
			name,
			profile_pic,
		})
	}
}

impl<S> OptionalFromRequestParts<S> for TailscaleUser
where
	S: Send + Sync,
{
	type Rejection = AppError;

	async fn from_request_parts(
		parts: &mut Parts,
		state: &S,
	) -> Result<Option<Self>, Self::Rejection> {
		<Self as FromRequestParts<S>>::from_request_parts(parts, state)
			.await
			.map(Some)
			.or_else(|err| {
				if let AppError::AuthMissingHeader(_) = err {
					Ok(None)
				} else {
					Err(err)
				}
			})
	}
}

#[derive(Debug, Clone)]
pub struct TailscaleAdmin(pub TailscaleUser);

impl<S> FromRequestParts<S> for TailscaleAdmin
where
	Db: FromRef<S>,
	Option<TailnetDirectory>: FromRef<S>,
	S: Send + Sync,
{
	type Rejection = AppError;

	async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
		let user = if cfg!(debug_assertions) {
			// The bypass grants admin, unless a test opts into the non-admin
			// path with the dev header — then behave like the real extractor
			// does for a non-admin: reject with insufficient permissions.
			if parts.headers.contains_key(DEV_NON_ADMIN_HEADER) {
				return Err(AppError::AuthInsufficientPermissions {
					required: "admin".into(),
				});
			}
			TailscaleUser {
				login: "admin@localhost".into(),
				name: "You".into(),
				profile_pic: None,
			}
		} else {
			let user =
				<TailscaleUser as FromRequestParts<S>>::from_request_parts(parts, state).await?;
			let mut db = Db::from_ref(state).get().await?;
			let directory = Option::<TailnetDirectory>::from_ref(state);
			if !user.is_admin(&mut db, directory.as_ref()).await? {
				return Err(AppError::AuthInsufficientPermissions {
					required: "admin".into(),
				});
			}
			user
		};

		// Cache the user's name + pic so endpoints that record human actions
		// (issue/incident resolve, notes) can render avatars without
		// round-tripping to Tailscale. Centralised here so every admin
		// handler gets it for free.
		let mut db = Db::from_ref(state).get().await?;
		CachedTailscaleUser::upsert(
			&mut db,
			&user.login,
			&user.name,
			user.profile_pic.as_deref(),
		)
		.await?;

		Ok(TailscaleAdmin(user))
	}
}
