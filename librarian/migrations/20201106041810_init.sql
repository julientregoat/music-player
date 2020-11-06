CREATE TABLE artists(
  id INTEGER PRIMARY KEY UNIQUE NOT NULL,
  name TEXT UNIQUE NOT NULL,
  created TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- TODO this should be UNIQUE(artist_id, release.name) but can't do that
CREATE TABLE releases (
  id INTEGER PRIMARY KEY UNIQUE NOT NULL,
  name TEXT NOT NULL,
  date TEXT,
  created TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- may make more sense to have a concept of primary artist, making it simpler
-- for naming dirs etc. this solves the UNIQUE artist + release name issue
CREATE TABLE artist_releases (
  artist_id INTEGER NOT NULL REFERENCES artists (id),
  release_id INTEGER NOT NULL REFERENCES releases (id),
  PRIMARY KEY (artist_id, release_id)
);

CREATE TABLE tracks (
  id INTEGER PRIMARY KEY UNIQUE NOT NULL,
  name TEXT NOT NULL,
  release_id INTEGER NOT NULL REFERENCES releases (id),
  file_path TEXT UNIQUE NOT NULL,
  channels INTEGER NOT NULL,
  sample_rate INTEGER NOT NULL,
  bit_rate INTEGER NOT NULL,
  track_num INTEGER NULL,
   -- TODO determine default empty array value or alternate encoding method
  tags TEXT NOT NULL DEFAULT '',
  created TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  modified TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
  -- UNIQUE(name, release_id) -- maybe
);

CREATE TRIGGER update_track_modified
  AFTER UPDATE ON tracks
  BEGIN
  UPDATE tracks SET modified = datetime('now') WHERE id = NEW.id;
  END;
