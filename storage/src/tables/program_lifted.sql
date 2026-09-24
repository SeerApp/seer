CREATE TABLE IF NOT EXISTS program_lifted (
    program_blob_hash BLOB NOT NULL CHECK (length(program_blob_hash) = 32),
    start_pc INTEGER NOT NULL,
    end_pc INTEGER NOT NULL CHECK (end_pc >= start_pc),
    blob_hash BLOB NOT NULL CHECK (length(blob_hash) = 32),
    PRIMARY KEY (program_blob_hash, start_pc),
    FOREIGN KEY (program_blob_hash) REFERENCES program (self_blob_hash) ON DELETE RESTRICT
);
