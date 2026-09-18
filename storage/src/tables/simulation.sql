CREATE TABLE IF NOT EXISTS simulation (
    transaction_blob_hash BLOB NOT NULL CHECK (length(transaction_blob_hash) = 32),
    state_blob_hash BLOB NOT NULL CHECK (length(state_blob_hash) = 32),
    created_at TEXT NOT NULL,
    created_in_dir TEXT NOT NULL,
    PRIMARY KEY (transaction_blob_hash, state_blob_hash)
);
