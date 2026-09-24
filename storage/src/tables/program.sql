CREATE TABLE IF NOT EXISTS program (
    self_blob_hash BLOB PRIMARY KEY CHECK (length(self_blob_hash) = 32),
    idl_blob_hash BLOB CHECK (idl_blob_hash IS NULL OR length(idl_blob_hash) = 32)
);
