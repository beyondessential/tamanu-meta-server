-- Seed the canopy-wide default schedule + retention for the well-known
-- `tamanu-postgres` backup type, so groups inherit a sane schedule out of the
-- box (the scheduler resolves per-(group,type) override → this default → floor).
-- Operators tune the canopy-wide default (here) and per-group overrides via the
-- UI. ON CONFLICT DO NOTHING so a hand-edited default isn't clobbered on replay.
-- auto_enable stays FALSE: capabilities remain opt-in (the operator enables a
-- type per server); this row only provides the inherited schedule/retention.
INSERT INTO backup_type_defaults (type, default_interval, default_retention, auto_enable)
VALUES (
    'tamanu-postgres',
    INTERVAL '6 hours',
    '{"keep_latest": 1, "keep_daily": 7, "keep_weekly": 4, "keep_monthly": 6, "keep_annual": 0}'::jsonb,
    false
)
ON CONFLICT (type) DO NOTHING;
