# In-flight backup progress and the snapshot moment

Scenarios verifying that a device can report progress while a backup is running,
that the operator can see whether a long run is actually moving data, and that a
backup's age is measured from when its data was frozen rather than when its upload
landed.

## Device contract — progress reporting

- [x] A grouped device bound to a live server can post progress and it is stored against the run — verifies spec: BAK
- [x] A device bound to no live server is refused with the distinct precondition status — verifies spec: BAK
- [x] An ungrouped server's device is refused with the distinct conflict status — verifies spec: BAK
- [x] Progress is accepted when the group's backup configuration is absent or not ready, and when the type is not a registered capability — the run is already under way, and refusing telemetry when a group is misconfigured hides exactly the case that needs watching — verifies spec: BAK
- [x] Counters a device does not measure stay unset rather than defaulting to zero, so "not reported" stays distinguishable from "nothing moved" — verifies spec: BAK
- [x] Engine-specific detail Canopy does not model is stored verbatim, nested structure intact — verifies spec: BAK
- [x] A device reporting faster than Canopy accepts gets the distinct rate-limit status, and the reports below the budget are all accepted — verifies spec: BAK
- [x] Progress for a run already reported complete is accepted rather than refused, so a report racing the completion is not an error — verifies spec: BAK
- [x] A sample can be recorded for a run that has no run row yet, which is the normal case — verifies spec: BAK

## The snapshot moment

- [x] A run's freeze moment survives when the report omits it, taken from the progress the run already reported — verifies spec: BAK
- [x] The moment is taken from the *first* sample that carried it, not the latest sample — a device announces it once, early, and omits it thereafter — verifies spec: BAK
- [x] A moment announced during the run wins over a different one repeated on the report (write-once, first value seen stands) — verifies spec: BAK
- [x] A run whose device never reports the moment leaves it unset — verifies spec: BAK

## Report backfill

- [x] A report omitting a transfer figure inherits it from the last progress sample, since counters are cumulative — verifies spec: BAK
- [x] A figure the report does supply always wins over the progress series — verifies spec: BAK
- [x] A run that reported no progress at all is recorded exactly as before, with omitted figures left unset — verifies spec: BAK

## Staleness measured from the data's age

- [x] A run reported recently but whose data was frozen much earlier is aged from the freeze moment — verifies spec: BKJ
- [x] A run that reported no freeze moment is aged from its report time, unchanged from prior behaviour — verifies spec: BKJ
- [x] Given a run reported later but carrying older data, both the single-server and the batch query select the run with the *newer data* — so a server's freshness never travels backwards as runs arrive — verifies spec: BKJ

## Progress series storage and pruning

- [x] A run's series reads oldest-first, and the latest-sample query returns the newest — verifies spec: BAK
- [x] The batch loaders key results by run and issue no query for an empty input — verifies spec: BKO
- [x] Pruning deletes samples past the cutoff and leaves fresh ones — verifies spec: BKJ

## Rate derivation

- [x] Rate is the difference of cumulative counters across the trailing window, not just the last pair — verifies spec: BKO
- [x] A dropped sample mid-window costs resolution but leaves the rate correct — verifies spec: BAK
- [x] A single sample yields no rate rather than zero — verifies spec: BKO
- [x] A window spanning no time yields no rate rather than an infinite one — verifies spec: BKO
- [x] A run reporting no uploaded figure yields no rate, and still shows its other figures — verifies spec: BKO
- [x] A device gone quiet keeps its last figures and shows an ever-growing gap since last contact — verifies spec: BKO

## Operator view

- [x] An in-flight run shows what it has transferred against the total it expects, with a progress bar and its current rate — verifies spec: BKO
- [x] An in-flight run's figures advance on their own while the page stays open, and its rate becomes derivable once a second sample lands — verifies spec: BKO
- [x] An in-flight run that has reported no progress still shows as in progress, with no figures invented and no freeze moment claimed — verifies spec: BKO
- [x] An in-flight row's expanded detail shows the engine's counters and sets the proxy's tally against them, surfacing any divergence and the protocol overhead — verifies spec: BKO
- [x] Raw engine data is behind a toggle and hidden until asked for — verifies spec: BKO
- [x] The rate chart plots a run's series, and says so plainly when there is too little to chart — verifies spec: BKO
- [x] The chart renders in both light and dark, stating its unit once on the axis with ticks as bare round numbers — verifies spec: BKO
- [x] A completed run shows the moment its data was frozen alongside its report time, and carries no live figures — verifies spec: BKO
- [x] A completed run that reported no freeze moment shows none — verifies spec: BKO
- [x] An issuance from a client predating run correlation shows as in flight with no figures, rather than picking up another run's progress — verifies spec: BKO

## Not covered

- [ ] Whether a stalled or silent run should raise an alert — deliberately out of scope; thresholds are to be chosen from real fleet series rather than guessed, so only visibility ships
- [ ] The bestool side of the contract — a separate handoff; this covers only what Canopy offers and accepts
