-- Outbox for Slack posts. The state-change call sites in `Incident::open_for`
-- and friends insert a row here inside their own transaction; the
-- `slacker_outbox` job binary drains it and does the actual HTTP. That
-- decouples "the database commit succeeded" from "Slack is reachable right
-- now", and keeps retry/backoff logic in one place.
--
-- Phase A only writes `incident_open` and `incident_resolve` rows and
-- delivers via the Slack Workflow webhook (no thread anchoring possible
-- there — the webhook never returns a `ts`). Phase B adds `issue_join`,
-- `issue_leave`, `incident_note` kinds and delivery via `chat.postMessage`,
-- and that's when `slack_threads` (a separate migration) starts mattering.
CREATE TABLE slack_outbox (
	id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
	kind         TEXT NOT NULL,
	incident_id  UUID NOT NULL REFERENCES incidents(id) ON DELETE CASCADE,
	issue_id     UUID REFERENCES issues(id) ON DELETE CASCADE,
	note_id      UUID REFERENCES incident_notes(id) ON DELETE CASCADE,
	payload      JSONB NOT NULL,
	delivered_at TIMESTAMPTZ,
	attempts     INTEGER NOT NULL DEFAULT 0,
	last_error   TEXT
);

-- The worker's hot query: pending rows in insertion order. Partial index
-- keeps it tight — once delivered_at is set the row is no longer scanned.
CREATE INDEX slack_outbox_pending
	ON slack_outbox (created_at)
	WHERE delivered_at IS NULL;
