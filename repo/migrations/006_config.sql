-- Configuration values (key-value store)
CREATE TABLE IF NOT EXISTS config_values (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    key TEXT NOT NULL UNIQUE,
    value TEXT,
    value_type TEXT NOT NULL DEFAULT 'string'
        CHECK (value_type IN ('string', 'integer', 'boolean', 'json')),
    description TEXT,
    scope TEXT NOT NULL DEFAULT 'global',
    scope_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Configuration change history
CREATE TABLE IF NOT EXISTS config_history (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    config_key TEXT NOT NULL,
    old_value TEXT,
    new_value TEXT,
    changed_by UUID REFERENCES users(id),
    changed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    reason TEXT
);

-- Campaign toggles (feature flags)
CREATE TABLE IF NOT EXISTS campaign_toggles (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    starts_at TIMESTAMPTZ,
    ends_at TIMESTAMPTZ,
    created_by UUID REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_config_values_key ON config_values(key);
CREATE INDEX IF NOT EXISTS idx_config_values_scope ON config_values(scope);
CREATE INDEX IF NOT EXISTS idx_config_history_config_key ON config_history(config_key);
CREATE INDEX IF NOT EXISTS idx_config_history_changed_at ON config_history(changed_at);
CREATE INDEX IF NOT EXISTS idx_campaign_toggles_name ON campaign_toggles(name);
CREATE INDEX IF NOT EXISTS idx_campaign_toggles_enabled ON campaign_toggles(enabled);
