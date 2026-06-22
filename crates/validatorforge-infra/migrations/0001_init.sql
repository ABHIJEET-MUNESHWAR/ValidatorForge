-- ValidatorForge schema: node read-model + range-partitioned run history.

-- Node read model: the current state of every node in the fleet.
CREATE TABLE IF NOT EXISTS nodes (
    id         TEXT        PRIMARY KEY,
    payload    JSONB       NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS nodes_updated_at_idx ON nodes (updated_at DESC);

-- Deployment run history: high-volume, append-mostly, time-series audit trail.
-- RANGE-partitioned by started_at so recent-window queries prune to a single
-- partition and old months can be detached/archived without a costly DELETE.
CREATE TABLE IF NOT EXISTS deployment_runs (
    id         TEXT        NOT NULL,
    target     TEXT        NOT NULL,
    status     TEXT        NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    payload    JSONB       NOT NULL,
    PRIMARY KEY (id, started_at)
) PARTITION BY RANGE (started_at);

-- A default partition catches rows outside any explicit monthly partition so
-- inserts never fail; a maintenance job promotes hot ranges to their own table.
CREATE TABLE IF NOT EXISTS deployment_runs_default
    PARTITION OF deployment_runs DEFAULT;

-- Example explicit monthly partitions (extend via a scheduled maintenance task).
CREATE TABLE IF NOT EXISTS deployment_runs_2025m06
    PARTITION OF deployment_runs
    FOR VALUES FROM ('2025-06-01') TO ('2025-07-01');

CREATE TABLE IF NOT EXISTS deployment_runs_2025m07
    PARTITION OF deployment_runs
    FOR VALUES FROM ('2025-07-01') TO ('2025-08-01');

CREATE INDEX IF NOT EXISTS deployment_runs_target_idx
    ON deployment_runs (target, started_at DESC);
