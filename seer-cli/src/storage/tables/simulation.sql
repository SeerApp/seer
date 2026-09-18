CREATE TABLE IF NOT EXISTS simulation (
    hash BLOB PRIMARY KEY CHECK (length(hash) = 32),
    created_at TEXT NOT NULL,
    created_in_dir TEXT NOT NULL,
    transaction_blob_hash BLOB NOT NULL CHECK (length(transaction_blob_hash) = 32),
    state_blob_hash BLOB NOT NULL CHECK (length(state_blob_hash) = 32)
);
