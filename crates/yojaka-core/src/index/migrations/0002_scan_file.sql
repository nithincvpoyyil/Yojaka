CREATE TABLE scan (
  id INTEGER PRIMARY KEY,
  root_path BLOB NOT NULL,
  started_at INTEGER NOT NULL,
  finished_at INTEGER,
  status TEXT NOT NULL,                    -- running|complete|partial|failed
  files_seen INTEGER,
  error_count INTEGER
);

CREATE TABLE file (
  id INTEGER PRIMARY KEY,
  full_path BLOB NOT NULL UNIQUE,         -- absolute, raw bytes
  parent_folder BLOB NOT NULL,
  file_name BLOB NOT NULL,
  extension TEXT,
  size_bytes INTEGER NOT NULL,
  modified_at_ns INTEGER NOT NULL,
  status_changed_at_ns INTEGER,
  created_at_ns INTEGER,
  device_id INTEGER NOT NULL,
  inode INTEGER NOT NULL,
  media_kind TEXT NOT NULL,                -- image|video
  format TEXT,
  is_cloud_only INTEGER NOT NULL DEFAULT 0,
  last_seen_scan_id INTEGER NOT NULL REFERENCES scan(id),
  metadata_status TEXT NOT NULL DEFAULT 'pending',
  partial_hash BLOB,
  full_hash BLOB,
  perceptual_hash INTEGER
);
CREATE INDEX file_size ON file(size_bytes);
CREATE INDEX file_full_hash ON file(full_hash);
CREATE INDEX file_parent ON file(parent_folder);
CREATE INDEX file_devino ON file(device_id, inode);
CREATE INDEX file_kind ON file(media_kind);

CREATE TABLE scan_error (
  scan_id INTEGER,
  file_path BLOB,
  error_kind TEXT,
  error_message TEXT
);
