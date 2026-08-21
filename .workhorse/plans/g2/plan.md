# Harvest contract: alertd checks filed by Canopy

Working notes for the contract between `bestool-alertd` running in the relay and what Canopy files under the `alertd` source. Gates N1 and informs the relay method set.

## What the crate already settles

Read against `bestool-alertd` 26.0.1 (`crates/alertd` in the bestool repo).

- **Parity is by construction, if Canopy does not re-derive it.** `perform_sweep(...) -> SweepResult` builds `payload["health"]` through the same `Check::to_wire()` a pushed bestool uses. A check's name is a `&'static str` in the registry and its detail fields are the same map, so a harvested filing matches a pushed one in check name and in the fields scoped policy reaches as `check.<field>`, provided the relay produces the payload via `perform_sweep` and Canopy ingests `health[]` verbatim through the same path a device push takes. The parity risk is Canopy re-modelling the filing on its side, and the arity problem below.
- **Thresholds are compile-time constants.** `WARN_PCT_USED`, `FAIL_DEPTH`, `FAIL_OLDEST_SECS` and friends live in each check's own Rust, read from no config, no env, no file. There is no per-server threshold surface for the harvest to supply: thresholds come from the crate version. Threshold configuration and version skew are therefore one question, not two.
- **Selection is a library feature.** `perform_sweep` takes `selected_names` / `skip_names`, validates them against the registry, and runs the selected checks concurrently.
- **`enable_heal` is a parameter.** The harvest passes it off, which suppresses the self-heal actions (`canopy_registration`, `fhir_jobs` restarting FHIR workers) that would otherwise fire against a host the relay does not own.
- **`get_or_create_server_id()` needs handling regardless.** `perform_sweep` calls it, and it reads or writes a host file to mint a `metaServerId`. The harvest must neither create nor depend on one: a Kubernetes server's identity is the operator's selection from L1's picker.

## The substrate, not a subset

Per-check eligibility already exists and already resolves to a skip. What the crate cannot express is a second axis: the registry's categories say **what inputs a check needs** (`@tamanu` an install, `@db` any database, `host` neither) and not **whose host the check speaks for**. A relay is a Linux process with a filesystem, memory, load and IPs of its own, so a host-subject check does not skip there. It runs and reports the relay pod's facts as the server's, identically for every instance that relay serves. That failure mode is worse than a failure, because it passes.

The answer is not to turn those checks off. Most of them are asking a question that has a real Kubernetes answer, and the check's own logic is already substrate-agnostic: `memory` reads a used-bytes and a total-bytes from `sysinfo`, computes a percentage, and grades it at 90 and 98. Only the acquisition is host-specific. Give the check a **substrate** to ask instead of reaching for the local machine, and the same graded logic reports on the workloads that constitute the server.

So eligibility becomes a three-way split rather than on or off.

### Portable once a substrate can answer

- `memory` — container working set against the container's memory limit. The limit is a better denominator than a VM's total, which carries cache-accounting noise.
- `disk_free` — the CNPG PVC's usage against `Cluster.spec.storage.size`, which J1 already identified for M1's storage check.
- `uptime` — pod start time and container restart count.
- `version_drift` — running container image tags against the deployment's version, which in Kubernetes is a more direct reading than parsing systemd units.
- `tamanu_service` — expected duties against running workloads.
- `tamanu_http` — the relay dialling the service ClusterIP rather than the config's canonical URL.

### The concept does not exist there, so a skip is the right answer

`btrfs` (EBS, no btrfs), `time_sync` (the node's clock is EKS's concern), `external_users` (no logins, and `pods/exec` is deliberately outside the relay's RBAC), `tailscale` and `tailscale_config` (a Kubernetes server is not a tailnet node), the `caddy_*` family and `caddyfile_version` (Gateway API, no Caddy), `held_captures` (bestool's own backup holds, and Kubernetes backups are out of scope per B1), `canopy_registration` (meaningless when Canopy files directly).

These are the checks that skip today by accident of what the relay image contains. Under the substrate model they skip for a stated reason instead, which is the same outcome reached honestly.

### A different check in Kubernetes

- `load` — a node load average says nothing about a pod. The Kubernetes analogue is CPU throttling, which is a different measurement and arguably a different check.
- `inodes` — PVC inode usage needs kubelet stats rather than the metrics API.
- `http_errors` — parses Caddy logs today; the analogue needs a log source the relay does not have.
- `billing_tags`, `munin`, `ips` — marginal value, no clean analogue.

## The arity problem, which is where parity actually bites

On a VM one host is one server, so `memory` is one `(used_bytes, total_bytes, percent_used)` triple. A Kubernetes server is several workloads (a central's tasks, sync and API replicas) each with its own limit, so the substrate's answer is naturally several triples. Same check name, so one catalog entry and one policy holds, but the detail fields diverge, and a scoped policy rule reading `check.percent_used` would mean different things on the two substrates. That is the card's parity worry, now with teeth, and it lands on detail rather than on naming.

The resolution that keeps parity is to make the check **instance-shaped on every substrate** and treat the VM as the degenerate one-instance case. CHK already models this ("Checks with instances"): one state per check, each instance graded through policy against its own detail, the effective result the most urgent across non-skipped instances, and the detail carrying every instance that is not passing. So the substrate interface should return a set of subjects, of size one on a VM, and the check's grading loop is written once. `Stat` values are never posted to Canopy, so the metrics endpoint does not constrain this.

## Where this collides with the K8S spec

K8S currently draws the line as infrastructure checks under `kubernetes` (Canopy reading the cluster) and database-derived checks under `alertd` (harvested). The substrate model moves that line: **workloads ready**, **restarts**, **storage** and **resource pressure** are the same conditions as a substrate-aware `tamanu_service`, `uptime`, `disk_free` and `memory`, and **server live** is the same condition as `tamanu_http`.

So the line is no longer "infrastructure versus database" but "what an alertd check can express portably versus what is genuinely Kubernetes-only". Whichever way it settles, it needs deciding before M1 and N1 both build their half, and it is a spec change to K8S rather than only a bestool change. The options are to shrink the `kubernetes` source to what has no VM counterpart (namespace presence, Gateway wiring, pod scheduling and eviction), or to keep both and accept two sources reporting the same condition under different names, which the fleet spread in FIG would show as duplicate checks.

Shrinking looks right on this card's own logic: a condition an operator configures once should not exist twice because the server runs on a different substrate.

## The dependency-direction fork

Two ways to make a check substrate-aware, and they trade the same property this card exists to protect.

- **Feature-gated Kubernetes support inside alertd.** The check and both its substrate implementations sit together in one crate, so a new check ships with both behaviours or neither, and version skew covers both at once. Cost is a `kube` dependency behind a feature, and the discipline to keep it out of the default build so unrelated binaries do not carry it.
- **A substrate trait in alertd, implemented by the relay.** Alertd gains no Kubernetes dependency at all and the client lives where it already is. But a check's Kubernetes behaviour and its VM behaviour then sit in different repos on different release cycles, so they can diverge, which is the exact failure the reuse exists to prevent.

The feature gate wins on the card's own reasoning, despite the bloat instinct pointing the other way. The anti-divergence property is the whole reason we are embedding alertd rather than reimplementing its checks.

## Consequences that follow from the substrate

- **The threshold constants become substrate-sensitive.** 90 and 98 percent were tuned for a VM's total memory; a container against a 512Mi limit may want different numbers, and disk at 80 and 95 percent means something different for a PVC that autoscales. So "where do thresholds come from" gets a second half: not a per-server surface, but whether one constant is right for both substrates, per check.
- **The relay's read set widens.** Container memory needs `metrics.k8s.io`, a different API group from the core objects J1 enumerated, and PVC usage needs kubelet stats or CSI metrics. That lands on H1's relay method-set card, and it argues for the check-shaped method surface there, since the alternative exports a metrics proxy to Canopy.
- **State-file collisions retire on their own.** `http_errors` and `external_users` persist state to one fixed path (`dirs::cache_dir()/bestool/doctor-*.json`), so several instances in one process would read and write each other's state. Both fall in the skip group, so the hazard goes without needing per-instance state paths. This, not `perform_sweep` being self-contained, is the answer to the re-entrancy question. If a portable check later needs persistent state, the substrate has to carry the state location too.

## Version skew and FIG

`perform_sweep` takes `binary_version` and it lands in the server-wide detail, which is where FIG reads the bestool figure. In the harvest that argument is naturally the relay's embedded alertd version.

Presenting it as the Kubernetes server's bestool version makes skew observable: an operator sees a harvested server on one alertd version beside pushed servers on another, in the fleet spread FIG already draws. The bound is then keeping the relay's `bestool-alertd` dependency tracking the fleet's shipped bestool, with a bump being a relay release on the cadence the relay's protocol versioning already needs. Under the feature-gate option this covers the substrate code too. Whether FIG needs a fold is open; on this reading it may not, since the figure keeps its definition and only its reporter changes.
