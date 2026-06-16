//! Shared library for the backup scheduler binaries: the k8s Job manifest
//! builder and the `JobSpawner` abstraction over the kube API. Kept here (not
//! in `commons-servers`) so `kube`/`k8s-openapi` stay off the crates that don't
//! spawn Jobs; the pure scheduler helpers (jitter, due-ness, billing,
//! retention floor) live in `commons_servers::backup_jobs`.

pub mod backup;
