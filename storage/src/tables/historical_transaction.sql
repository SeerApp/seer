CREATE TABLE IF NOT EXISTS historical_transaction (
    transaction_blob_hash BLOB NOT NULL CHECK (length(transaction_blob_hash) = 32),
    state_blob_hash BLOB NOT NULL CHECK (length(state_blob_hash) = 32),
    network TEXT NOT NULL CHECK (network IN ('mainnet', 'testnet', 'devnet')),
    sig BLOB NOT NULL CHECK (length(sig) = 64),
    PRIMARY KEY (transaction_blob_hash, state_blob_hash),
    FOREIGN KEY (transaction_blob_hash, state_blob_hash) REFERENCES simulation (transaction_blob_hash, state_blob_hash) ON DELETE CASCADE
);
