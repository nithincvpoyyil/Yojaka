-- Per-file metadata (Iteration 2). One row per `file` row; deleted with it.
CREATE TABLE meta (
  file_id INTEGER PRIMARY KEY REFERENCES file(id) ON DELETE CASCADE,
  width INTEGER,
  height INTEGER,
  taken_at INTEGER,                  -- unix seconds of the wall-clock time as recorded (no timezone applied)
  taken_at_src TEXT NOT NULL,        -- exif|quicktime|container|none
  camera_make TEXT,
  camera_model TEXT,
  lens TEXT,
  orientation INTEGER,
  has_gps INTEGER NOT NULL DEFAULT 0,
  gps_lat REAL,
  gps_lon REAL,
  -- reverse-geocoded place (filled by the geocode pass, cached)
  place_country_code TEXT,           -- ISO 3166-1 alpha-2
  place_country TEXT,
  place_state TEXT,                  -- admin1
  place_district TEXT,               -- admin2
  place_city TEXT,                   -- nearest populated place
  place_dist_km REAL,                -- distance to that place
  loc_src TEXT,                      -- gps|pair|inferred (NULL = no location); pair = borrowed from the Live Photo partner
  frames INTEGER,                    -- gif / animated webp / apng frame count
  -- video-only columns (NULL for images)
  duration_ms INTEGER,
  fps REAL,
  rotation INTEGER,                  -- 0|90|180|270 from the track matrix
  video_codec TEXT,                  -- hevc|h264|prores|av1|vp9|...
  audio_codec TEXT,
  bitrate INTEGER,                   -- bits/s (size*8/duration)
  container TEXT,
  -- Live Photo pairing
  content_id TEXT,                   -- Apple content identifier, if present
  pair_id INTEGER                    -- same value on both halves of a Live Photo
);
CREATE INDEX meta_taken ON meta(taken_at);
CREATE INDEX meta_pair ON meta(pair_id);
CREATE INDEX meta_content_id ON meta(content_id);
CREATE INDEX meta_place ON meta(place_country_code, place_state, place_city);
