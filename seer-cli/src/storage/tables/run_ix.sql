CREATE TABLE IF NOT EXISTS run_ix (
    run_id INTEGER NOT NULL,
    ix INTEGER NOT NULL CHECK (ix >= 0),
    trace_blob_hash BLOB NOT NULL CHECK (length(trace_blob_hash) = 32),
    PRIMARY KEY (run_id, ix),
    FOREIGN KEY (run_id) REFERENCES run (id) ON DELETE CASCADE
);
