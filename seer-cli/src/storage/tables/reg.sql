CREATE TABLE IF NOT EXISTS reg (
    id INTEGER PRIMARY KEY,
    run_id INTEGER NOT NULL,
    ix INTEGER NOT NULL,
    start_pc INTEGER NOT NULL,
    end_pc INTEGER NOT NULL,
    self_blob_hash BLOB NOT NULL CHECK (length(self_blob_hash) = 32),
    program_blob_hash BLOB NOT NULL CHECK (length(program_blob_hash) = 32),
    pubkey BLOB NOT NULL CHECK (length(pubkey) = 32),
    FOREIGN KEY (run_id, ix) REFERENCES run_ix (run_id, ix) ON DELETE CASCADE,
    FOREIGN KEY (program_blob_hash) REFERENCES program (self_blob_hash) ON DELETE RESTRICT
);
