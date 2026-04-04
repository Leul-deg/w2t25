-- Commerce additions: shipping/points columns on orders, config seeds, low-stock threshold

-- Add shipping fee and points earned to orders
ALTER TABLE orders ADD COLUMN IF NOT EXISTS shipping_fee_cents INTEGER NOT NULL DEFAULT 0;
ALTER TABLE orders ADD COLUMN IF NOT EXISTS points_earned    INTEGER NOT NULL DEFAULT 0;

-- Raise the default low-stock threshold to 10 (spec: alert below 10 units)
ALTER TABLE inventory ALTER COLUMN low_stock_threshold SET DEFAULT 10;

-- Seed default commerce config values (idempotent)
INSERT INTO config_values (id, key, value, value_type, description, scope)
VALUES
    (gen_random_uuid(), 'shipping_fee_cents',    '695', 'integer',
     'Default shipping fee charged per order, in cents ($6.95)', 'global'),
    (gen_random_uuid(), 'points_rate_per_dollar', '1',  'integer',
     'Points earned per whole dollar of order subtotal (default: 1 point per $1.00)', 'global')
ON CONFLICT (key) DO NOTHING;

-- Seed default campaign toggles (idempotent)
INSERT INTO campaign_toggles (id, name, description, enabled)
VALUES
    (gen_random_uuid(), 'store_enabled',   'Enable the merch store for all users',    TRUE),
    (gen_random_uuid(), 'points_enabled',  'Enable points earning on purchases',       TRUE),
    (gen_random_uuid(), 'free_shipping',   'Offer free shipping (when configured)',    FALSE)
ON CONFLICT (name) DO NOTHING;

-- Index to support the auto-close scheduler query efficiently
CREATE INDEX IF NOT EXISTS idx_orders_pending_created
    ON orders(created_at) WHERE status = 'pending';
