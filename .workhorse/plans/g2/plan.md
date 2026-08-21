# Harvest contract: alertd checks filed by Canopy

Working notes for the contract between `bestool-alertd` running in the relay and what Canopy files under the `alertd` source. Gates N1 and informs the relay method set.

## What the crate already settles

Read against `bestool-alertd` 26.0.1 (`crates/alertd` in the bestool repo).

- **Parity is by construction, if Canopy does not re-derive it.** `perform_sweep(...) -> SweepResult` builds `payload["health"]` through the same `Check::to_wire()` a pushed bestool uses. A check's name is a `&'static str` in the registry and its detail fields are the same map, so a harvested filing matches a pushed one in check name and in the fields scoped policy reaches as `check.<field>`, provided the relay produces the payload via `perform_sweep` and Canopy ingests `health[]` verbatim through the same path a device push takes. The parity risk is entirely Canopy re-modelling the filing on its side.
- **Thresholds are compile-time constants.** `FAIL_ERRORS`, `WARN_DEPTH`, `FAIL_OLDEST_SECS` and friends live in each check's own Rust, read from no config, no env, no file. So there is no per-server threshold surface for the harvest to supply: thresholds come from the crate version. Threshold configuration and version skew are therefore one question, not two.
- **Selection is a library feature.** `perform_sweep` takes `selected_names` / `skip_names`, validates them against the registry, and runs the selected checks concurrently.
- **`enable_heal` is a parameter.** The harvest passes it off, which suppresses the self-heal actions (`canopy_registration`, `fhir_jobs` restarting FHIR workers) that would otherwise fire against a host the relay does not own.

## The eligibility gap

Per-check eligibility already exists and already resolves to a skip, which is the right mechanism: a skip carries no signal, does not count in the health rollup, and does not age into broken. A curated DB-only allowlist is the wrong answer because it drifts, and it drifts in Canopy rather than next to the check.

The gap is that the registry's categories encode **what inputs a check needs** (`@tamanu` a Tamanu install, `@db` any database, `host` neither) and not **whose host the check speaks for**. The harvest needs the second axis and the crate cannot currently express it. A relay is a Linux process with a filesystem, memory, load, IPs and a tailnet identity of its own, so a host-subject check does not skip there: it runs and reports the relay pod's facts as the server's.

That failure mode is worse than a failure. It passes, and it passes identically for every instance the relay serves.

### Host-subject checks that report the relay's facts as the server's

`disk_free`, `memory`, `load` (node load average), `uptime` (feeds the `uptimeSecs` fact), `ips` (feeds the IP facts), `munin`, `tailscale_config`, `external_users`, `http_errors`, `canopy_registration`.

### Host-subject checks that skip today, for fragile reasons

`time_sync` (no `timedatectl` in the image), `btrfs` (no `btrfs` binary, and it shells through `sudo`), `inodes` (no `df`), `held_captures` (no hold-records directory), `billing_tags` (no cached Canopy tags, no IMDS). These are correct only by accident of what the relay image contains. Adding busybox to the image starts `inodes` reporting the relay's filesystems.

### Host-subject checks that the `@tamanu` category hides

`version_drift` and `tamanu_service` gate on supervisor detection (systemd or pm2), `caddyfile_version` reads the Tamanu Caddyfile, `tamanu_http` has no eligibility gate at all and probes the config's canonical URL. So the axis cuts across the existing categories rather than lining up with the `host` arm. `fhir_config` is the well-behaved case: it gates on `ctx.has_install` and skips cleanly.

### Consequences that fall out of the same fix

- **State-file collisions retire with it.** `http_errors` and `external_users` persist state to a single fixed path (`dirs::cache_dir()/bestool/doctor-*.json`), so driving several instances concurrently in one process makes them read and write each other's state. Both are host-subject, so a correct skip removes the hazard rather than needing per-instance state paths. This, not `perform_sweep` being self-contained, is the real answer to the re-entrancy question.
- **`get_or_create_server_id()` needs handling regardless.** `perform_sweep` calls it, and it reads or writes a host file to mint a `metaServerId`. The harvest must not create or depend on one: a Kubernetes server's identity is the operator's selection from L1's picker.

## Decisions to make

- **Where the axis lives and what shape it takes.** Preference is a context fact in the crate (the process is not the subject's host) that the registry consults, so a new check ships with correct harvest behaviour instead of waiting on a Canopy-side list. Cost is a bestool change: the fact, the registry arms that consult it, and re-categorising the dozen-odd entries above along the host-subject axis. That change is a prerequisite for N1.
- **Whether the harvest files skips at all.** Filing the full sweep gives a Kubernetes server a check list where most entries are skips explaining it has no disk of its own, and CHK already presents a silenced check as skipped, so the two read alike. Dropping them at the relay and filing only checks that ran looks cleaner, and per CHK's reporting semantics an omitted check that was never reported is simply absent.
- **`tamanu_http` against a Kubernetes server.** It duplicates the `kubernetes` source's **Server live** check, so it either skips as host-subject or is deliberately allowed to overlap.

## Version skew and FIG

`perform_sweep` takes `binary_version` and it lands in the server-wide detail, which is where FIG reads the bestool figure. In the harvest that argument is naturally the relay's embedded alertd version.

Presenting it as the Kubernetes server's bestool version makes skew observable: an operator sees a harvested server on one alertd version sitting beside pushed servers on another, in the same fleet spread FIG already draws. The bound is then keeping the relay's `bestool-alertd` dependency tracking the fleet's shipped bestool, with a bump being a relay release on the cadence the relay's protocol versioning already needs. Whether FIG needs a fold for this is open; on this reading it may not, since the figure keeps its current definition and only its reporter changes.
