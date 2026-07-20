-- Work queue for deferred incident (re-)evaluation.
--
-- The device status-ingest path records the status row and per-check issue
-- state synchronously, then enqueues the server here instead of evaluating
-- incident membership inline. Inline evaluation took a per-group
-- `server_groups FOR UPDATE` lock and held it for the whole push, serialising
-- every check-in for the group and, under load, convoying the fleet. The
-- monitor pod drains this queue and runs the (single-writer) evaluation off
-- the request path.
--
-- `server_id` is the primary key: at most one pending re-evaluation per
-- server, so a burst of pushes coalesces into one unit of work.
CREATE TABLE incident_reeval_queue (
    server_id UUID PRIMARY KEY REFERENCES servers (id) ON DELETE CASCADE,
    enqueued_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
