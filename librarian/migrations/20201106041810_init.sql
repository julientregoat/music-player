-- NB: sqlite allows for null primary keys

CREATE TABLE artists (
  id INTEGER PRIMARY KEY NOT NULL,
  name TEXT UNIQUE NOT NULL,
  created TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE releases (
  id INTEGER PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  date TEXT,
  created TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- may make more sense to have a concept of primary artist, making it simpler
-- for naming dirs etc. this solves the UNIQUE artist + release name issue
CREATE TABLE artist_releases (
  artist_id INTEGER NOT NULL,
  release_id INTEGER NOT NULL,
  FOREIGN KEY(artist_id) REFERENCES artists(id),
  FOREIGN KEY(release_id) REFERENCES releases(id),
  PRIMARY KEY(artist_id, release_id)
);

CREATE TABLE tracks (
  id INTEGER PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  release_id INTEGER NOT NULL,
  file_path TEXT UNIQUE NOT NULL,
  channels INTEGER NOT NULL,
  sample_rate INTEGER NOT NULL,
  bit_depth INTEGER NOT NULL,
  track_num INTEGER NULL,
  created TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  modified TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (release_id) REFERENCES releases(id)
);

CREATE TRIGGER update_track_modified
AFTER UPDATE ON tracks
BEGIN
  UPDATE tracks
  SET modified = datetime ('now')
  WHERE id = NEW.id;
END;

CREATE TABLE tags (
  id INTEGER PRIMARY KEY NOT NULL,
  name VARCHAR(25) NOT NULL COLLATE NOCASE -- too long, too short?
);

CREATE TABLE track_tags (
  track_id INTEGER NOT NULL,
  tag_id INTEGER NOT NULL,
  created TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY(track_id) REFERENCES tracks(id),
  FOREIGN KEY(tag_id) REFERENCES tags(id),
  PRIMARY KEY(track_id, tag_id)
);