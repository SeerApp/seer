CREATE TABLE IF NOT EXISTS program (
    self_blob_hash BLOB PRIMARY KEY CHECK (length(self_blob_hash) = 32),
    disasm_blob_hash BLOB CHECK (disasm_blob_hash IS NULL OR length(disasm_blob_hash) = 32),
    lifted_blob_hash BLOB CHECK (lifted_blob_hash IS NULL OR length(lifted_blob_hash) = 32)
);
