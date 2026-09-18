CREATE TABLE IF NOT EXISTS run (
    id INTEGER PRIMARY KEY,
    simulation_id BLOB NOT NULL CHECK (length(simulation_id) = 32),
    run_at TEXT NOT NULL,
    run_in_dir TEXT NOT NULL,
    overrides TEXT NOT NULL DEFAULT '{}',
    keep_on_crash INTEGER NOT NULL DEFAULT 0 CHECK (keep_on_crash IN (0, 1)),
    error TEXT,
    FOREIGN KEY (simulation_id) REFERENCES simulation (hash) ON DELETE CASCADE
);
