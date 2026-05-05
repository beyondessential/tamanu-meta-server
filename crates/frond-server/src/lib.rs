// frond-server: a QUIC endpoint for canopy speaking the bes.canopy/1 protocol.
//
// See `docs/plans/frond-server.md` for the staged build-out.

pub mod keys;
pub mod server;
pub mod tls;

pub use server::{accept_loop, bind};

/// ALPN negotiated for frond-server QUIC connections. Bumping this string is
/// the lever for incompatible protocol revisions.
pub const ALPN: &[u8] = b"bes.canopy/1";
