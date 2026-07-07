//! Parsing and formatting of human-friendly units: byte sizes (1024-based,
//! displayed in Kubernetes notation like `20Gi`) and durations (jiff's
//! "friendly" format like `2h 30m`). Raw values remain whole bytes and whole
//! seconds; these helpers only translate at the operator-facing edge.

use jiff::{Span, SpanRelativeTo, SpanRound, Unit};

/// A human-unit string failed to parse.
#[derive(Debug, Clone, thiserror::Error)]
pub enum UnitParseError {
	/// Not a valid byte size.
	#[error(
		"invalid size {input:?}: expected a whole number of bytes with an optional 1024-based unit, e.g. `20Gi` or `512Mi`"
	)]
	Bytes {
		/// The rejected input.
		input: String,
	},
	/// Not a valid duration.
	#[error("invalid duration {input:?}: {reason}")]
	Duration {
		/// The rejected input.
		input: String,
		/// Why it was rejected.
		reason: String,
	},
}

/// Parse a byte size with an optional unit suffix into a raw byte count.
///
/// Accepts a whole number followed by an optional `k`/`m`/`g` unit, itself
/// optionally followed by `i` and/or `b` in any capitalisation (`20g`, `20G`,
/// `20Gi`, `20GB`, `20GiB` are all the same). Every unit is 1024-based: `20G`
/// means 20Gi in Kubernetes notation, never 20×10⁹. A bare number (or a `b`
/// suffix) is a plain byte count.
pub fn parse_bytes(input: &str) -> Result<i64, UnitParseError> {
	let err = || UnitParseError::Bytes {
		input: input.into(),
	};
	let trimmed = input.trim();
	let digits_end = trimmed
		.find(|c: char| !c.is_ascii_digit())
		.unwrap_or(trimmed.len());
	let (digits, suffix) = trimmed.split_at(digits_end);
	if digits.is_empty() {
		return Err(err());
	}
	let value: i64 = digits.parse().map_err(|_| err())?;
	let multiplier: i64 = match suffix.trim_start().to_ascii_lowercase().as_str() {
		"" | "b" => 1,
		"k" | "ki" | "kb" | "kib" => 1 << 10,
		"m" | "mi" | "mb" | "mib" => 1 << 20,
		"g" | "gi" | "gb" | "gib" => 1 << 30,
		_ => return Err(err()),
	};
	value.checked_mul(multiplier).ok_or_else(err)
}

/// Format a raw byte count in Kubernetes notation (`20Gi`, `512Mi`, `1Ki`),
/// using the largest 1024-based unit that divides it exactly; counts that
/// don't divide evenly (and zero) stay as plain byte numbers.
pub fn format_bytes(bytes: i64) -> String {
	if bytes > 0 {
		for (factor, unit) in [(1 << 30, "Gi"), (1 << 20, "Mi"), (1 << 10, "Ki")] {
			if bytes % factor == 0 {
				return format!("{}{unit}", bytes / factor);
			}
		}
	}
	bytes.to_string()
}

/// Parse a duration in jiff's "friendly" format (or ISO 8601) into a whole
/// number of seconds.
///
/// Accepts e.g. `2h 30m`, `90m`, `1d 12h`, `2 hours 30 minutes`; days and
/// weeks are fixed at 24 hours and 7 days. Rejects negative durations,
/// sub-second precision, calendar units (months and up), and bare numbers
/// without a unit.
pub fn parse_duration_seconds(input: &str) -> Result<i64, UnitParseError> {
	let err = |reason: String| UnitParseError::Duration {
		input: input.into(),
		reason,
	};
	let span: Span = input
		.trim()
		.parse()
		.map_err(|e: jiff::Error| err(e.to_string()))?;
	let duration = span
		.to_duration(SpanRelativeTo::days_are_24_hours())
		.map_err(|e| err(e.to_string()))?;
	if duration.is_negative() {
		return Err(err("must not be negative".into()));
	}
	if duration.subsec_nanos() != 0 {
		return Err(err("must be a whole number of seconds".into()));
	}
	Ok(duration.as_secs())
}

/// Format a whole number of seconds in jiff's "friendly" format, balanced up
/// to days (24-hour days): `9000` → `2h 30m`, `129600` → `1d 12h`.
pub fn format_duration_seconds(seconds: i64) -> String {
	Span::new()
		.try_seconds(seconds)
		.and_then(|span| {
			span.round(
				SpanRound::new()
					.largest(Unit::Day)
					.relative(SpanRelativeTo::days_are_24_hours()),
			)
		})
		.map(|span| format!("{span:#}"))
		.unwrap_or_else(|_| format!("{seconds}s"))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn bytes_parse_all_suffix_spellings_as_1024_based() {
		const GI: i64 = 1 << 30;
		for input in [
			"20g", "20G", "20gi", "20Gi", "20gb", "20GB", "20gib", "20GiB",
		] {
			assert_eq!(parse_bytes(input).unwrap(), 20 * GI, "input {input:?}");
		}
		assert_eq!(parse_bytes("512Mi").unwrap(), 512 << 20);
		assert_eq!(parse_bytes("3k").unwrap(), 3 << 10);
		assert_eq!(parse_bytes("3KiB").unwrap(), 3 << 10);
	}

	#[test]
	fn bytes_parse_bare_and_b_suffixed_numbers_as_bytes() {
		assert_eq!(parse_bytes("20").unwrap(), 20);
		assert_eq!(parse_bytes("20b").unwrap(), 20);
		assert_eq!(parse_bytes("20B").unwrap(), 20);
		assert_eq!(parse_bytes("0").unwrap(), 0);
		assert_eq!(parse_bytes(" 20 Gi ").unwrap(), 20 << 30);
	}

	#[test]
	fn bytes_parse_rejects_junk() {
		for input in [
			"", "g", "20t", "20TB", "-20g", "20.5g", "20gg", "20 gi b", "abc", "20girib",
		] {
			assert!(parse_bytes(input).is_err(), "input {input:?}");
		}
		// Overflows i64.
		assert!(parse_bytes("9999999999999999999").is_err());
		assert!(parse_bytes("9999999999g").is_err());
	}

	#[test]
	fn bytes_format_uses_largest_exact_unit() {
		assert_eq!(format_bytes(20 << 30), "20Gi");
		assert_eq!(format_bytes(512 << 20), "512Mi");
		assert_eq!(format_bytes(1 << 10), "1Ki");
		assert_eq!(format_bytes(2048 << 20), "2Gi");
		assert_eq!(format_bytes(1536), "1536");
		assert_eq!(format_bytes(0), "0");
		assert_eq!(format_bytes(999), "999");
	}

	#[test]
	fn bytes_round_trip() {
		for n in [0, 999, 1536, 1 << 10, 512 << 20, 20 << 30] {
			assert_eq!(parse_bytes(&format_bytes(n)).unwrap(), n, "value {n}");
		}
	}

	#[test]
	fn duration_parse_friendly_and_iso() {
		assert_eq!(parse_duration_seconds("2h 30m").unwrap(), 9000);
		assert_eq!(parse_duration_seconds("2h30m").unwrap(), 9000);
		assert_eq!(parse_duration_seconds("90m").unwrap(), 5400);
		assert_eq!(parse_duration_seconds("1d 12h").unwrap(), 129600);
		assert_eq!(parse_duration_seconds("1w").unwrap(), 604800);
		assert_eq!(parse_duration_seconds("2 hours 30 minutes").unwrap(), 9000);
		assert_eq!(parse_duration_seconds("PT2H30M").unwrap(), 9000);
		assert_eq!(parse_duration_seconds("0s").unwrap(), 0);
	}

	#[test]
	fn duration_parse_rejects_junk() {
		for input in [
			"", "banana", "20", "1mo", "1y", "-1h", "1h ago", "0.5s", "1s 500ms",
		] {
			assert!(parse_duration_seconds(input).is_err(), "input {input:?}");
		}
	}

	#[test]
	fn duration_format_balances_up_to_days() {
		assert_eq!(format_duration_seconds(0), "0s");
		assert_eq!(format_duration_seconds(45), "45s");
		assert_eq!(format_duration_seconds(90), "1m 30s");
		assert_eq!(format_duration_seconds(3600), "1h");
		assert_eq!(format_duration_seconds(9000), "2h 30m");
		assert_eq!(format_duration_seconds(86400), "1d");
		assert_eq!(format_duration_seconds(129600), "1d 12h");
		assert_eq!(format_duration_seconds(604800), "7d");
	}

	#[test]
	fn duration_round_trip() {
		for n in [0, 45, 90, 3600, 9000, 86400, 129600, 604800] {
			assert_eq!(
				parse_duration_seconds(&format_duration_seconds(n)).unwrap(),
				n,
				"value {n}"
			);
		}
	}
}
