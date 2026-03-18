use commons_errors::{AppError, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{kind::ServerKind, rank::ServerRank};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaTicket {
	/// Ticket format version, must be "ticket-1".
	pub v: String,
	/// The server's own UUID.
	pub server_id: Uuid,
	/// The server's ECDSA public key in SubjectPublicKeyInfo PEM format.
	pub public_key: String,
	/// Human-readable hostname.
	pub hostname: String,
	/// Tailscale IP address.
	pub tailscale_ip: Option<String>,
	/// Tailscale DNS name.
	pub tailscale_name: Option<String>,
	/// The canonical HTTPS URL for the server.
	pub canonical_url: String,
	/// Hosting type (e.g. "kvm", "ec2").
	pub hosting: Option<String>,
	/// Server kind hint, if provided by the ticket.
	pub kind: Option<ServerKind>,
	/// Server rank hint, if provided by the ticket.
	pub rank: Option<ServerRank>,
	/// The public key of the parent (central) server, in SubjectPublicKeyInfo PEM format.
	pub central_public_key: Option<String>,
}

impl MetaTicket {
	/// Decode a base64-encoded ticket string.
	pub fn from_base64(input: &str) -> Result<Self> {
		use base64::Engine as _;
		let json = base64::prelude::BASE64_STANDARD
			.decode(input.trim())
			.map_err(|e| AppError::custom(format!("Invalid base64 in ticket: {e}")))?;
		let ticket: Self = serde_json::from_slice(&json)
			.map_err(|e| AppError::custom(format!("Invalid ticket JSON: {e}")))?;
		if ticket.v != "ticket-1" {
			return Err(AppError::custom(format!(
				"Unsupported ticket version: {}",
				ticket.v
			)));
		}
		Ok(ticket)
	}

	/// Extract the raw SubjectPublicKeyInfo DER bytes from the PEM public key.
	pub fn public_key_der(&self) -> Result<Vec<u8>> {
		Self::pem_to_der(&self.public_key)
	}

	/// Decode a SubjectPublicKeyInfo PEM string to raw DER bytes.
	pub fn pem_to_der(pem: &str) -> Result<Vec<u8>> {
		use base64::Engine as _;
		let body = pem
			.trim()
			.lines()
			.filter(|l| !l.starts_with("-----"))
			.collect::<Vec<_>>()
			.join("");
		base64::prelude::BASE64_STANDARD
			.decode(&body)
			.map_err(|e| AppError::custom(format!("Invalid base64 in public key: {e}")))
	}
}
