-- Capture each device's stable Tailscale node identity, so private-server
-- can authenticate tailnet-resident callers on the /public/... mount by
-- mapping their CGNAT / ULA source IP back to a Device row via the
-- Tailscale control plane API.
--
-- All three columns are nullable: existing mTLS-only devices have no
-- Tailscale identity, and a device may keep operating without one
-- indefinitely. The UNIQUE constraint plus partial index allows arbitrarily
-- many NULLs while preventing two devices from claiming the same node id.
ALTER TABLE devices
    ADD COLUMN tailscale_node_id   TEXT UNIQUE,
    ADD COLUMN tailscale_node_name TEXT,
    ADD COLUMN tailscale_tailnet   TEXT;

CREATE INDEX devices_tailscale_node_id_idx ON devices (tailscale_node_id)
    WHERE tailscale_node_id IS NOT NULL;
