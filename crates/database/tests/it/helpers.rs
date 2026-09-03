//! Fixtures shared across the integration tests.

use commons_types::{namespace::Namespace, server::app_type::ApplicationType};

/// One application type's namespace.
///
/// Most catalog tests exercise grading, liveness or scoping rather than the
/// namespacing itself, and the names they use (`disk_space`, `x`) are
/// application-subject ones, so this is the namespace ingest would file them
/// in. Tests that are *about* the namespacing name their namespaces inline.
pub fn app_ns() -> Namespace {
	Namespace::Application(ApplicationType::TamanuCentral)
}
