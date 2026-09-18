CREATE TABLE IF NOT EXISTS glassbox (
    run_id INTEGER NOT NULL,
    ix INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    glassbox_blob_hash BLOB NOT NULL CHECK (length(glassbox_blob_hash) = 32),
    PRIMARY KEY (run_id, ix),
    FOREIGN KEY (run_id, ix) REFERENCES run_ix (run_id, ix) ON DELETE CASCADE
);
