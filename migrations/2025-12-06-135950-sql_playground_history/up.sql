CREATE TABLE sql_playground_history (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    query TEXT NOT NULL,
    tailscale_user TEXT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_sql_playground_history_created_at ON sql_playground_history (created_at DESC);
CREATE INDEX idx_sql_playground_history_tailscale_user ON sql_playground_history (tailscale_user);
CREATE INDEX idx_sql_playground_history_user_created_at ON sql_playground_history (tailscale_user, created_at DESC);
