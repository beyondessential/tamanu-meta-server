//! Whose problem a failed conversation with a certificate authority is.
//!
//! Lives here rather than beside the ACME client because both sides need it: the
//! client classifies, and the alerting decides what to report from the
//! classification. Which of those two a failure belongs to is the whole
//! distinction the alerting rests on.
// spec: CRT#when-issuance-fails

use serde::{Deserialize, Serialize};

/// What went wrong when Canopy talked to the authority.
///
/// Canopy's own inability to issue is not any one server's fault and is reported
/// against Canopy; an order that failed on its own merits is reported against the
/// server that asked for it. Telling them apart matters because they reach
/// different people — a deployment's certificate running out is that deployment's
/// problem to notice, and Canopy being unable to issue at all is Canopy's.
///
/// Ordered by how far the blame reaches, so the worst thing a round of orders hit
/// is `max()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityFault {
	/// This order's own problem: the name would not validate, the request was
	/// refused, a zone write failed. The authority is working.
	Order,
	/// The authority could not be reached at all.
	Unreachable,
	/// The authority was reached but Canopy's account with it is not usable.
	Account,
	/// The authority was reached and told Canopy to slow down. Its limits are
	/// shared across every group whose domain sits in the same zone, so running
	/// them down is fleet-wide rather than one group's.
	Throttled,
}

impl AuthorityFault {
	/// Whether this is Canopy's to report rather than the asking server's.
	pub fn is_canopys(self) -> bool {
		!matches!(self, Self::Order)
	}
}
