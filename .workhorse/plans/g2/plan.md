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

- `disk_free` — the CNPG PVC's usage against `Cluster.spec.storage.size`, which J1 already identified for M1's storage check. A PVC has a real declared size, so the percentage means what it means on a VM.
- `uptime` — pod start time and container restart count.
- `version_drift` — running container image tags against the deployment's version, which in Kubernetes is a more direct reading than parsing systemd units.
- `tamanu_service` — expected duties against running workloads.
- `tamanu_http` — the relay dialling the service ClusterIP rather than the config's canonical URL.

### The concept does not exist there, so a skip is the right answer

`btrfs` (EBS, no btrfs), `time_sync` (the node's clock is EKS's concern), `external_users` (no logins, and `pods/exec` is deliberately outside the relay's RBAC), `tailscale` and `tailscale_config` (a Kubernetes server is not a tailnet node), the `caddy_*` family and `caddyfile_version` (Gateway API, no Caddy), `held_captures` (bestool's own backup holds, and Kubernetes backups are out of scope per B1), `canopy_registration` (meaningless when Canopy files directly).

These are the checks that skip today by accident of what the relay image contains. Under the substrate model they skip for a stated reason instead, which is the same outcome reached honestly.

### A different check in Kubernetes

- `memory` — **no denominator exists.** The central and facility API containers declare `requests` only and set no memory `limits` (`central/specs.ts`, `facility/specs.ts`; only the web container sets them), so those pods are Burstable and can grow to node capacity. There is no per-container ceiling to take a percentage of, and the ceiling that does exist is the node's, shared with every other pod on it including other servers' and other namespaces'. The condition worth alerting on is the OOM kill or the eviction, which is an event rather than a gauge, and it is already the `kubernetes` source's **Resource pressure** check.
- `load` — a node load average says nothing about a pod, and CPU is likewise requests-only here. Same answer as `memory`: throttling and eviction are events, not a gauge.
- `inodes` — PVC inode usage needs kubelet stats rather than the metrics API.
- `billing_tags`, `munin`, `ips` — marginal value, no clean analogue.

`http_errors` looked like it belonged here and does not: see below.

## `http_errors` against Envoy Gateway

The check reads Caddy's admin `/metrics` (Prometheus text) for `caddy_http_request_duration_seconds_count` by status code, snapshots the counters to disk each run, deltas against the oldest snapshot inside a 10 minute window, and grades the 5xx share at 5 and 20 percent. Only the acquisition is Caddy-specific, so Envoy is a direct substitute.

What the deployment actually looks like (`pulumi/k8s-essentials/envoyGateway.ts`, Envoy Gateway v1.8.3):

- **`mergeGateways: true`**, set on the `EnvoyProxy` the GatewayClass points at. So every Gateway in the cluster merges onto **one** Envoy fleet of 2 replicas in `envoy-gateway-system`, labelled `bes.gateway.role: proxy`. Not one proxy per server, and not even one per namespace.
- `ingress()` is called once per server, so each central and each facility gets its own Gateway plus five HTTPRoutes named `<server>-frontend`, `<server>-api`, `<server>-api-legacy`, `<server>-api-import`, `<server>-api-import-legacy`, all in the deployment's namespace.
- Proxy metrics are on port 19001 at `/stats/prometheus`, enabled by default, in the same Prometheus text format the check already parses.

Attribution therefore comes from the stat labels, not from which pod is scraped: `envoy_cluster_upstream_rq_xx{envoy_response_code_class="5", envoy_cluster_name="httproute/<namespace>/<route>/rule/<n>"}`. Namespace plus route-name prefix identifies the server and the suffix identifies the duty, so the harvested check can report per-server and can separate frontend from API from import, which the Caddy version on a VM cannot. The 4xx and 5xx split comes from the class label rather than from parsing status codes. `routeStatName` would add per-route virtual-host stats but is disabled by default and we do not need it.

Three complications, and they are the interesting part:

- **One scrape serves every server in the cluster.** A per-instance sweep must not scrape 19001 itself, or a tick costs (instances x replicas) scrapes of identical data. The reading is cluster-scoped and partitioned by label, then fanned out per server.
- **Counters are per pod, and the pods roll.** `counters_reset` rejects any baseline where a counter went down, which is correct for one local Caddy but wrong for a 2 replica fleet: when one pod rolls, the summed counter drops, every baseline in the window is rejected, and the check falls back to its 10 second in-run sample. The proxy pods tolerate spot (`CAN_RUN_ON_SPOT`, and prefer it), so that is frequent rather than rare. Snapshots need keying by (pod, cluster, class) with vanished pods dropped rather than read as a negative delta.
- **The snapshot history has nowhere to live.** State goes to `dirs::cache_dir()`, which in a relay pod is ephemeral and shared across every instance the relay serves.

So this check forces two axes into the substrate interface beyond "where do I read this number": **the scope at which a reading is shared** (per workload, per server, per cluster) and **where the check's own persistent state lives**. Better to design those in from the start than to discover them on the second check that needs them.

It also adds a second skew axis. The check would depend on Envoy Gateway's stat naming, which is an implementation detail of a component with its own version (pinned in ops, and the upstream issue asking for friendlier labels suggests it may move). If the expected series are absent the check cannot run, so that is **broken**, not failed, which matches the existing precedent where a class 42 SQL error reports broken rather than blaming the deployment.

## Arity turns out not to be a problem

The worry was that a VM answers with one value where a Kubernetes server answers with one per workload, so the detail fields diverge and a policy rule reading `check.percent_used` means different things on the two substrates. Checked against the actual checks, that case barely arises.

- **The multi-valued checks are already multi-valued on a VM.** A VM Tamanu also runs several services, so `version_drift` already carries `.with_detail("instances", <array>)` and `tamanu_service` already carries `.with_detail("diagnostics", <array>)`. Kubernetes workloads slot into the same arrays with no shape change at all.
- **The single-valued host-resource checks either have a real per-subject denominator or do not port.** `disk_free` has one (the PVC's declared size). `memory` and `load` do not (requests-only containers, no ceiling), so they do not port and their concern is covered as an event by **Resource pressure**.

So no instance-shaping of the VM side, and no need for CHK's "checks with instances" machinery here. Reach for it only if per-workload policy grading turns out to be wanted, which can happen later without changing the check name or its primary fields.

Where a check does aggregate several subjects into one number, **it must aggregate by worst and not by sum.** A sum hides the failure: one full PVC beside an empty one of the same size reads as half full, and the thing that is out of space is invisible. Taking the worst subject's value keeps the single-number detail shape, so parity holds exactly, and naming the worst subject in an extra detail field costs nothing, because a rule reading `check.percent_used` still means the same thing on both substrates and the VM simply has no such extra field.

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
- **The relay's read set widens, and stops being per-namespace.** Container memory needs `metrics.k8s.io`, a different API group from the core objects J1 enumerated, and PVC usage needs kubelet stats or CSI metrics. `http_errors` needs to list pods in `envoy-gateway-system` and scrape a port on them, which is outside the Tamanu namespaces entirely: read-only and no `exec` or `portforward`, but it breaks the assumption that a relay reads only the namespaces of the servers it serves. That lands on H1's relay method-set card, and it argues for the check-shaped method surface there, since the alternative exports a metrics proxy to Canopy.
- **State-file collisions retire on their own.** `http_errors` and `external_users` persist state to one fixed path (`dirs::cache_dir()/bestool/doctor-*.json`), so several instances in one process would read and write each other's state. Both fall in the skip group, so the hazard goes without needing per-instance state paths. This, not `perform_sweep` being self-contained, is the answer to the re-entrancy question. If a portable check later needs persistent state, the substrate has to carry the state location too.

## Version skew and FIG

`perform_sweep` takes `binary_version` and it lands in the server-wide detail, which is where FIG reads the bestool figure. In the harvest that argument is naturally the relay's embedded alertd version.

Presenting it as the Kubernetes server's bestool version makes skew observable: an operator sees a harvested server on one alertd version beside pushed servers on another, in the fleet spread FIG already draws. The bound is then keeping the relay's `bestool-alertd` dependency tracking the fleet's shipped bestool, with a bump being a relay release on the cadence the relay's protocol versioning already needs. Under the feature-gate option this covers the substrate code too. Whether FIG needs a fold is open; on this reading it may not, since the figure keeps its definition and only its reporter changes.
