CREATE TABLE IF NOT EXISTS historical_transaction (
    hash BLOB PRIMARY KEY CHECK (length(hash) = 32),
    network TEXT NOT NULL CHECK (network IN ('mainnet', 'testnet', 'devnet')),
    sig BLOB NOT NULL CHECK (length(sig) = 64),
    FOREIGN KEY (hash) REFERENCES simulation (hash) ON DELETE CASCADE
);
