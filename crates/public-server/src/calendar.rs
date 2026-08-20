//! The internet-facing calendar feed of planned upgrades, gated by a token in
//! the URL ([`database::calendar_tokens::CalendarToken`]).
//!
//! Spec: `.workhorse/specs/private-server/upgrade-plans.md` (id `UPG`), "The
//! calendar feed".
//!
//! Calendar clients fetch a subscription URL unattended and cannot be asked for
//! a header, so the credential travels in the path. This is deliberately NOT
//! part of [`crate::routes`]: it must not appear on the private server's
//! `/public` nest nor in the device OpenAPI spec. The binary's `main` and the
//! test harness compose it in alongside the device routes.

use std::collections::HashMap;
use std::time::Duration;

use axum::{
	Router,
	extract::{Path, State},
	http::header,
	response::IntoResponse,
	routing::get,
};
use axum_client_ip::ClientIp;
use commons_errors::{AppError, Result};
use database::{
	calendar_tokens::CalendarToken, server_groups::ServerGroup, upgrade_plans::UpgradePlan,
	versions::Version,
};
use jiff::{SignedDuration, Timestamp, civil::Date, tz::TimeZone};
use uuid::Uuid;

use crate::state::AppState;

/// Failed-lookup budget within a 1-minute window, per source IP. Only failures
/// spend it, so a subscribed client never approaches it; a URL-guesser is
/// blunted. Guessing is hopeless anyway (tokens are 256-bit CSPRNG), this just
/// bounds the DB lookups it can burn.
const RL_WINDOW: Duration = Duration::from_secs(60);
const RL_PER_IP: u32 = 30;

/// How long a client is told to wait before re-fetching. Advisory: the big
/// calendar services poll on their own schedule regardless.
const REFRESH: &str = "PT1H";

/// How long an event runs when a plan names the hour it starts. A plan records
/// no duration, so the event marks the start of the window rather than claiming
/// to know when it ends.
const EVENT_LENGTH: SignedDuration = SignedDuration::from_hours(1);

/// The `/calendar` mount: the feed behind the URL-token gate.
pub fn routes(state: AppState) -> Router<()> {
	Router::new()
		.route("/calendar/{token}/upgrades.ics", get(feed))
		.with_state(state)
}

/// Serve the planned-upgrades calendar for a token.
///
/// Unknown and revoked tokens are both a plain 404: there is nothing at that
/// URL, and a guesser learns nothing from the answer.
// spec: UPG#the-calendar-feed
async fn feed(
	State(state): State<AppState>,
	ClientIp(ip): ClientIp,
	Path(presented): Path<String>,
) -> Result<impl IntoResponse> {
	let key = format!("calendar:{ip}");
	if state.rate_limiter.exceeded(&key, RL_PER_IP, RL_WINDOW) {
		tracing::warn!(target: "calendar_auth", %ip, "calendar auth rate limit exceeded");
		return Err(AppError::RateLimited);
	}

	let mut conn = state.db.get().await?;
	let Some(token) = CalendarToken::find_active(&mut conn, &presented).await? else {
		tracing::warn!(target: "calendar_auth", %ip, "calendar request with unusable token");
		if !state.rate_limiter.check(&key, RL_PER_IP, RL_WINDOW) {
			return Err(AppError::RateLimited);
		}
		return Err(AppError::NotFound("no such calendar".into()));
	};

	tracing::info!(token = %token.name, %ip, "calendar request");
	CalendarToken::touch_last_used(&mut conn, token.id).await?;

	let body = render(&mut conn).await?;
	Ok((
		[
			(header::CONTENT_TYPE, "text/calendar; charset=utf-8"),
			(
				header::CONTENT_DISPOSITION,
				"inline; filename=\"canopy-upgrades.ics\"",
			),
		],
		body,
	))
}

/// Build the whole calendar document.
async fn render(conn: &mut diesel_async::AsyncPgConnection) -> Result<String> {
	let plans = UpgradePlan::dated(conn).await?;

	let group_ids: Vec<Uuid> = plans.iter().map(|plan| plan.group_id).collect();
	let groups: HashMap<Uuid, ServerGroup> = ServerGroup::list_by_ids(conn, &group_ids)
		.await?
		.into_iter()
		.map(|group| (group.id, group))
		.collect();
	// Including drafts: a target yanked since the plan was recorded still has to
	// render as the version the deployment is going to.
	let versions: HashMap<Uuid, String> = Version::get_all_including_drafts(conn)
		.await?
		.into_iter()
		.map(|version| (version.id, version.as_semver().to_string()))
		.collect();

	let stamp = Timestamp::now();
	let mut out = String::new();
	line(&mut out, "BEGIN:VCALENDAR");
	line(&mut out, "VERSION:2.0");
	line(&mut out, "PRODID:-//BES//Canopy//EN");
	line(&mut out, "CALSCALE:GREGORIAN");
	line(&mut out, "METHOD:PUBLISH");
	line(&mut out, "X-WR-CALNAME:Canopy upgrades");
	line(
		&mut out,
		"X-WR-CALDESC:Where each deployment is going, and when.",
	);
	line(
		&mut out,
		&format!("REFRESH-INTERVAL;VALUE=DURATION:{REFRESH}"),
	);
	line(&mut out, &format!("X-PUBLISHED-TTL:{REFRESH}"));

	for plan in plans {
		let (Some(group), Some(target)) = (
			groups.get(&plan.group_id),
			versions.get(&plan.target_version_id),
		) else {
			continue;
		};
		let Some(date) = plan.planned_for else {
			continue;
		};
		event(&mut out, &plan, group, target, date, stamp)?;
	}

	line(&mut out, "END:VCALENDAR");
	Ok(out)
}

/// One planned upgrade.
fn event(
	out: &mut String,
	plan: &UpgradePlan,
	group: &ServerGroup,
	target: &str,
	date: Date,
	stamp: Timestamp,
) -> Result<()> {
	let done = plan.met_at.is_some();

	line(out, "BEGIN:VEVENT");
	line(out, &format!("UID:{}@canopy", plan.id));
	line(out, &format!("DTSTAMP:{}", utc(stamp)));
	line(
		out,
		&format!(
			"LAST-MODIFIED:{}",
			utc(plan.amended_at.unwrap_or(plan.created_at))
		),
	);

	match (plan.planned_time, plan.planned_zone.as_deref()) {
		(Some(time), Some(zone)) => {
			// Resolved to an instant rather than emitted as a wall clock with a
			// TZID: the feed carries no VTIMEZONE, and every client agrees on
			// what an instant means.
			let zoned = date
				.to_datetime(time)
				.to_zoned(TimeZone::get(zone).map_err(|e| AppError::custom(e.to_string()))?)
				.map_err(|e| AppError::custom(e.to_string()))?;
			let start = zoned.timestamp();
			let end = start
				.checked_add(EVENT_LENGTH)
				.map_err(|e| AppError::custom(e.to_string()))?;
			line(out, &format!("DTSTART:{}", utc(start)));
			line(out, &format!("DTEND:{}", utc(end)));
		}
		_ => {
			let end = date
				.tomorrow()
				.map_err(|e| AppError::custom(e.to_string()))?;
			line(out, &format!("DTSTART;VALUE=DATE:{}", day(date)));
			line(out, &format!("DTEND;VALUE=DATE:{}", day(end)));
		}
	}

	let summary = if done {
		format!("{} upgraded to {target}", group.name)
	} else {
		format!("{} upgrade to {target}", group.name)
	};
	line(out, &format!("SUMMARY:{}", escape(&summary)));

	let mut description = Vec::new();
	if let Some(met_at) = plan.met_at {
		description.push(format!("Reached {}", met_at.strftime("%Y-%m-%d")));
	} else if let Some(running) = &group.effective_version {
		description.push(format!("Now on {running}"));
	}
	if let Some(note) = plan
		.note
		.as_deref()
		.map(str::trim)
		.filter(|n| !n.is_empty())
	{
		description.push(note.to_owned());
	}
	if let Some(by) = &plan.created_by {
		description.push(format!("Planned by {by}"));
	}
	if !description.is_empty() {
		line(
			out,
			&format!("DESCRIPTION:{}", escape(&description.join("\n"))),
		);
	}

	// A planned upgrade is something to know about, not something that makes
	// whoever subscribed unavailable.
	line(out, "TRANSP:TRANSPARENT");
	line(out, "END:VEVENT");
	Ok(())
}

fn utc(at: Timestamp) -> String {
	at.strftime("%Y%m%dT%H%M%SZ").to_string()
}

fn day(date: Date) -> String {
	date.strftime("%Y%m%d").to_string()
}

/// Escape a text value per RFC 5545: backslash, semicolon, comma, and newline
/// carry structural meaning in a property value.
fn escape(value: &str) -> String {
	let mut out = String::with_capacity(value.len());
	for ch in value.chars() {
		match ch {
			'\\' => out.push_str("\\\\"),
			';' => out.push_str("\\;"),
			',' => out.push_str("\\,"),
			'\n' => out.push_str("\\n"),
			'\r' => {}
			ch => out.push(ch),
		}
	}
	out
}

/// Append one content line, folded to 75 octets as RFC 5545 requires. The
/// limit counts bytes but a break may only fall between characters, so a
/// multi-byte character moves to the next line whole.
fn line(out: &mut String, content: &str) {
	const LIMIT: usize = 75;

	let mut used = 0;
	for ch in content.chars() {
		let width = ch.len_utf8();
		if used + width > LIMIT {
			// The continuation space counts toward the next line's limit.
			out.push_str("\r\n ");
			used = 1;
		}
		out.push(ch);
		used += width;
	}
	out.push_str("\r\n");
}
