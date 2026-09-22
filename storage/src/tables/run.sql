CREATE TABLE IF NOT EXISTS run (
    id INTEGER PRIMARY KEY,
    transaction_blob_hash BLOB NOT NULL CHECK (length(transaction_blob_hash) = 32),
    state_blob_hash BLOB NOT NULL CHECK (length(state_blob_hash) = 32),
    run_at TEXT NOT NULL,
    run_in_dir TEXT NOT NULL,
    environment TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'finished')),
    error TEXT,
    parent_id INTEGER REFERENCES run (id) ON DELETE SET NULL,
    patches TEXT NOT NULL DEFAULT '[]',
    source TEXT NOT NULL DEFAULT '',
    FOREIGN KEY (transaction_blob_hash, state_blob_hash) REFERENCES simulation (transaction_blob_hash, state_blob_hash) ON DELETE CASCADE
);
