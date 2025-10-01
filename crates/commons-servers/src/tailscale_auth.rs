use axum::extract::{FromRef, FromRequestParts, OptionalFromRequestParts};
use commons_errors::AppError;
use database::{Db, admins::Admin};
use diesel_async::AsyncPgConnection;
use http::request::Parts;

const TAILSCALE_USER_LOGIN: &str = "Tailscale-User-Login";
const TAILSCALE_USER_NAME: &str = "Tailscale-User-Name";
const TAILSCALE_USER_PROFILE_PIC: &str = "Tailscale-User-Profile-Pic";

#[derive(Debug, Clone)]
pub struct TailscaleUser {
	pub login: String,
	pub name: String,
	pub profile_pic: Option<String>,
}

impl TailscaleUser {
	pub async fn is_admin(&self, db: &mut AsyncPgConnection) -> Result<bool, AppError> {
		Admin::check_email(db, &self.login).await
	}
}

impl<S> FromRequestParts<S> for TailscaleUser
where
	S: Send + Sync,
{
	type Rejection = AppError;

	async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
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
	S: Send + Sync,
{
	type Rejection = AppError;

	async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
		let user = <TailscaleUser as FromRequestParts<S>>::from_request_parts(parts, state).await?;
		let mut db = Db::from_ref(state).get().await?;
		if user.is_admin(&mut db).await? {
			Ok(TailscaleAdmin(user))
		} else {
			Err(AppError::AuthInsufficientPermissions {
				required: "admin".into(),
			})
		}
	}
}
