DROP INDEX devices_tailscale_node_id_idx;
ALTER TABLE devices
    DROP COLUMN tailscale_tailnet,
    DROP COLUMN tailscale_node_name,
    DROP COLUMN tailscale_node_id;
