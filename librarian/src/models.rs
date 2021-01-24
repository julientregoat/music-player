// use chrono::{DateTime, Utc};
use super::parse;
use core::borrow::BorrowMut;
use sqlx::{pool::PoolConnection, sqlite::Sqlite};
use std::collections::{hash_map, HashMap};
use tokio_stream::StreamExt;

pub type SqlitePoolConn = PoolConnection<Sqlite>;
pub type RowId = i64;
// type Timestamptz = DateTime<Utc>; // TODO use sqlx chrono ext to repl String?

const SQLITE_UNIQUE_VIOLATION: &'static str = "2067";

// TODO refactor exposed API? associated functions doesn't feel ideal
// sqlx examples show models organized as traits implemented on the Connection
// type. this is more ergonomic, but unneeded runtime work?
// maybe the move is to have a dao with the base structure, that returns
// any of the derived structures? need better naming for derived struct fns
// TODO use constant for table names in queries

#[derive(Debug)]
pub struct CollectionBase {
    id: RowId,
    name: String,
    created: String,
}

pub struct Collection<'c> {
    id: RowId,
    name: String,
    created: String,
    artists: HashMap<RowId, Artist>,
    releases: HashMap<RowId, Release<'c>>,
    tracks: Vec<Track<'c>>,
    // tags: Vec<Tag>, // TODO
}

// not sure this should be separate or exposed as is
impl CollectionBase {
    pub async fn create(
        conn: &mut SqlitePoolConn,
        name: &str,
    ) -> Result<RowId, sqlx::Error> {
        let id = sqlx::query("INSERT INTO collections (name) VALUES (?)")
            .bind(name)
            .execute(conn)
            .await?
            .last_insert_rowid();

        Ok(id)
    }
}

impl<'c> Collection<'c> {
    pub async fn load(
        conn: &mut SqlitePoolConn,
    ) -> Result<Collection<'c>, sqlx::Error> {
        // load all artists into memory, then releases, then tracks and link refs
        let artists = Artist::get_all(conn).await?;
        let releases = Release::get_all(conn, &artists).await?;
        unimplemented!()
    }
}

#[derive(Clone, Debug)]
pub struct Artist {
    pub id: RowId,
    pub name: String,
    pub created: String, // TODO parse date
}

impl Artist {
    pub async fn create(
        conn: &mut SqlitePoolConn,
        name: &str,
    ) -> Result<RowId, sqlx::Error> {
        sqlx::query("INSERT INTO artists (name) VALUES (?);")
            .bind(name)
            .execute(conn.borrow_mut())
            .await
            .map(|d| d.last_insert_rowid())
    }

    pub async fn get(
        conn: &mut SqlitePoolConn,
        id: RowId,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as!(Self, "SELECT * FROM artists WHERE id = ?", id)
            .fetch_one(conn)
            .await
    }

    pub async fn get_all(
        conn: &mut SqlitePoolConn,
    ) -> Result<HashMap<RowId, Self>, sqlx::Error> {
        let mut qstream =
            sqlx::query_as!(Self, "SELECT * FROM artists ORDER BY id ASC")
                .fetch(conn);
        let mut artists = HashMap::new();

        while let Some(Ok(a)) = qstream.next().await {
            artists.insert(a.id, a);
        }

        Ok(artists)
    }

    pub async fn find_by_name(
        conn: &mut SqlitePoolConn,
        name: &str,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as!(Self, "SELECT * FROM artists WHERE name = ?", name)
            .fetch_one(conn)
            .await
    }

    pub async fn get_release_artists(
        conn: &mut SqlitePoolConn,
        release_id: RowId,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(
            Self,
            "SELECT * from artists
            WHERE id IN (
                SELECT artist_id
                FROM artist_releases
                WHERE release_id = ?
            )",
            release_id
        )
        .fetch_all(conn)
        .await
    }
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct ReleaseBase {
    pub id: RowId,
    pub name: String,
    pub date: Option<String>,
    pub created: String, // TODO parse date
}
#[derive(Debug)]
pub struct Release<'c> {
    pub id: RowId,
    pub name: String,
    pub artists: Vec<&'c Artist>,
    pub date: Option<String>,
    pub created: String, // TODO parse date
}

impl<'c> Release<'c> {
    pub async fn get_all(
        conn: &mut SqlitePoolConn,
        artists: &'c HashMap<RowId, Artist>,
    ) -> Result<HashMap<RowId, Release<'c>>, sqlx::Error> {
        let mut qstream = sqlx::query!(
            "SELECT
                releases.id,
                releases.name,
                releases.date,
                releases.created,
                artist_releases.artist_id
            FROM releases
            JOIN artist_releases ON releases.id = artist_releases.release_id
            ORDER BY releases.id ASC"
        )
        .fetch(conn);
        let mut releases = HashMap::new();

        while let Some(Ok(r)) = qstream.next().await {
            let artist = artists.get(&r.artist_id).unwrap();

            match releases.entry(r.id) {
                hash_map::Entry::Vacant(entry) => {
                    entry.insert(Release {
                        id: r.id,
                        name: r.name,
                        artists: vec![artist],
                        date: r.date,
                        created: r.created,
                    });
                }
                hash_map::Entry::Occupied(entry) => {
                    let release = entry.into_mut();
                    release.artists.push(artist);
                }
            };
        }

        Ok(releases)
    }
}

#[derive(Debug)]
pub struct OwnedRelease {
    pub id: RowId,
    pub name: String,
    pub artists: Vec<Artist>,
    pub date: Option<String>,
    pub created: String, // TODO parse date
}

impl ReleaseBase {
    pub async fn create(
        conn: &mut SqlitePoolConn,
        name: &str,
        date: Option<&str>,
        artist_ids: &[RowId],
    ) -> Result<RowId, sqlx::Error> {
        let mut conn = conn;
        let release_id = sqlx::query(
            "INSERT INTO releases (name, date)
            VALUES (?, ?)",
        )
        .bind(name)
        .bind(date)
        .execute(conn.borrow_mut())
        .await?
        .last_insert_rowid();

        for id in artist_ids {
            ArtistRelease::create(&mut conn, *id, release_id).await?;
        }

        Ok(release_id)
    }

    pub async fn get(
        conn: &mut SqlitePoolConn,
        id: RowId,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as!(Self, "SELECT * FROM releases WHERE id = ?", id)
            .fetch_one(conn)
            .await
    }

    pub async fn get_artist_releases(
        conn: &mut SqlitePoolConn,
        artist_id: RowId,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(
            Self,
            "SELECT * from releases
             WHERE id IN (
                SELECT release_id
                FROM artist_releases
                WHERE artist_id = ?
            )",
            artist_id
        )
        .fetch_all(conn)
        .await
    }

    pub fn complete(self, artists: Vec<&Artist>) -> Release {
        Release {
            id: self.id,
            name: self.name,
            artists,
            date: self.date,
            created: self.created,
        }
    }

    pub fn complete_owned(self, artists: Vec<Artist>) -> OwnedRelease {
        OwnedRelease {
            id: self.id,
            name: self.name,
            artists,
            date: self.date,
            created: self.created,
        }
    }
}

// does this need to be pub? only used internally
#[derive(Clone, Debug)]
pub struct ArtistRelease {
    pub artist_id: RowId,
    pub release_id: RowId,
}

impl ArtistRelease {
    async fn create(
        conn: &mut SqlitePoolConn,
        artist_id: RowId,
        release_id: RowId,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO artist_releases (artist_id, release_id)
            VALUES (?, ?);",
        )
        .bind(artist_id)
        .bind(release_id)
        .execute(conn)
        .await
        .map(|_done| ())
    }
}

#[derive(Debug)]
pub struct Tag {
    pub id: RowId,
    pub name: String,
}

impl Tag {
    pub async fn create(
        conn: &mut SqlitePoolConn,
        name: &str,
    ) -> Result<RowId, sqlx::Error> {
        sqlx::query("INSERT INTO tags (name) VALUES (?)")
            .bind(name)
            .execute(conn)
            .await
            .map(|done| done.last_insert_rowid())
    }

    // tags will usually be reused instead of created -> optimize for the read
    pub async fn find_or_create(
        conn: &mut SqlitePoolConn,
        tag_name: &str,
    ) -> Result<RowId, sqlx::Error> {
        match sqlx::query_as!(
            Self,
            "SELECT id, name from tags WHERE name = ?",
            tag_name
        )
        .fetch_one(conn.borrow_mut())
        .await
        {
            Ok(tag) => Ok(tag.id),
            Err(sqlx::Error::Database(e)) => match e.code() {
                Some(code) if code == SQLITE_UNIQUE_VIOLATION => {
                    Self::create(conn, tag_name).await
                }
                Some(code) => panic!("tag insert db failure {:?}", code),
                None => panic!("tag insert db fail no code"),
            },
            Err(e) => panic!("tag insert failed {:?}", e),
        }
    }
}

#[derive(Debug)]
pub struct TrackTag {
    track_id: RowId,
    tag_id: RowId,
}

impl TrackTag {
    pub async fn create(
        conn: &mut SqlitePoolConn,
        track_id: RowId,
        tag_id: RowId,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT INTO track_tags (track_id, tag_id) VALUES (?, ?)")
            .bind(track_id)
            .bind(tag_id)
            .execute(conn)
            .await
            .map(|_done| ())
    }
}

// TODO store track duration
#[derive(Clone, Debug)]
pub struct TrackBase {
    pub id: RowId,
    pub name: String,
    pub release_id: RowId,
    pub file_path: String,
    pub channels: i64,
    pub sample_rate: i64,
    pub bit_depth: i64,
    pub track_num: Option<i64>,
    pub created: String,  // TODO parse date
    pub modified: String, // TODO parse date
}

pub struct Track<'c> {
    pub id: RowId,
    pub name: String,
    pub release: &'c Release<'c>,
    pub tags: Vec<&'c Tag>,
    pub file_path: String,
    pub channels: i64,
    pub sample_rate: i64,
    pub bit_depth: i64,
    pub track_num: Option<i64>,
    pub created: String,  // TODO parse date
    pub modified: String, // TODO parse date
}

// TODO how significant of an impact on memory does duplicating Artist, Release,
// and Tags per track have? performance?
// - use [A]RC to reference count and share memory
// - for Vec<DetailedTrack> etc,  wrap + store Artists/Releases/Tags and use
// refs in this struct
// - stack allocated str (e.g inlinable_string) for performance
#[derive(Debug)]
pub struct OwnedTrack {
    pub id: RowId,
    pub name: String,
    pub release: OwnedRelease,
    pub tags: Vec<Tag>,
    pub file_path: String,
    pub channels: i64,
    pub sample_rate: i64,
    pub bit_depth: i64,
    pub track_num: Option<i64>,
    pub created: String,  // TODO parse date
    pub modified: String, // TODO parse date
}

impl TrackBase {
    pub async fn create(
        conn: &mut SqlitePoolConn,
        name: &str,
        release_id: RowId,
        collection_id: RowId,
        file_path: &str,
        channels: i64,
        sample_rate: i64,
        bit_depth: i64,
        track_num: Option<i64>,
    ) -> Result<RowId, sqlx::Error> {
        let track_id = sqlx::query(
            "INSERT INTO tracks
            (
                name,
                release_id,
                collection_id,
                file_path,
                channels,
                sample_rate,
                bit_depth,
                track_num
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(name)
        .bind(release_id)
        .bind(collection_id)
        .bind(file_path)
        .bind(channels)
        .bind(sample_rate)
        .bind(bit_depth)
        .bind(track_num)
        .execute(conn.borrow_mut())
        .await?
        .last_insert_rowid();

        Ok(track_id)
    }

    pub async fn get(
        conn: &mut SqlitePoolConn,
        id: RowId,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as!(Self, "SELECT * FROM tracks WHERE id = ?", id)
            .fetch_one(conn)
            .await
    }

    pub async fn get_all(
        conn: &mut SqlitePoolConn,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(Self, "SELECT * FROM tracks;")
            .fetch_all(conn)
            .await
    }
}

impl OwnedTrack {
    // TODO should this be outside the Track impl? perhaps on Collection
    pub async fn get_all(
        conn: &mut SqlitePoolConn,
    ) -> Result<Vec<OwnedTrack>, sqlx::Error> {
        let tracks_with_releases = sqlx::query!(
            "SELECT
                    tracks.id,
                    tracks.name,
                    tracks.file_path,
                    tracks.channels,
                    tracks.sample_rate,
                    tracks.bit_depth,
                    tracks.track_num,
                    tracks.created,
                    tracks.modified,
                    releases.id as release_id,
                    releases.name as release_name,
                    releases.date as release_date,
                    releases.created as release_created
                FROM tracks
                JOIN releases ON tracks.release_id = releases.id;",
        )
        .fetch_all(conn.borrow_mut())
        .await?;

        let mut track_tags: HashMap<RowId, Vec<Tag>> = HashMap::new();
        sqlx::query!(
            "SELECT
                    track_tags.track_id,
                    tags.id,
                    tags.name
                FROM track_tags
                JOIN tags ON track_tags.tag_id = tags.id"
        )
        .fetch_all(conn.borrow_mut())
        .await?
        .into_iter()
        .for_each(|row| {
            let ra = track_tags.entry(row.track_id).or_insert(Vec::new());
            ra.push(Tag {
                id: row.id,
                name: row.name,
            })
        });

        let mut release_artists: HashMap<RowId, Vec<Artist>> = HashMap::new();
        sqlx::query!(
            "SELECT
                    artist_releases.release_id,
                    artists.id,
                    artists.name,
                    artists.created
                FROM artist_releases
                JOIN artists ON artist_releases.artist_id = artists.id;"
        )
        .fetch_all(conn.borrow_mut())
        .await?
        .into_iter()
        .for_each(|row| {
            let ra =
                release_artists.entry(row.release_id).or_insert(Vec::new());
            ra.push(Artist {
                id: row.id,
                name: row.name,
                created: row.created,
            })
        });

        let mut detailed_tracks = Vec::new();

        for track in tracks_with_releases {
            let dt = OwnedTrack {
                id: track.id,
                name: track.name,
                release: OwnedRelease {
                    id: track.release_id,
                    name: track.release_name,
                    date: track.release_date,
                    created: track.release_created,
                    artists: release_artists
                        .get(&track.release_id)
                        .unwrap()
                        .to_owned(),
                },
                // TODO is removing slower than cloning & dropping?
                tags: track_tags.remove(&track.id).unwrap(),
                file_path: track.file_path,
                channels: track.channels,
                sample_rate: track.sample_rate,
                bit_depth: track.bit_depth,
                track_num: track.track_num,
                created: track.created,
                modified: track.modified,
            };
            detailed_tracks.push(dt)
        }

        Ok(detailed_tracks)
    }
}

struct PlaylistFolder {
    id: RowId,
    name: String,
    created: String,
}

impl PlaylistFolder {
    pub async fn create(
        conn: &mut SqlitePoolConn,
        name: &str,
    ) -> Result<RowId, sqlx::Error> {
        let id = sqlx::query("INSERT INTO playlist_folders (name) VALUES (?)")
            .bind(name)
            .execute(conn)
            .await?
            .last_insert_rowid();

        Ok(id)
    }
}

struct Playlist {
    id: RowId,
    name: String,
    folder_id: Option<RowId>, // may not have parent folder
    created: String,
}

impl Playlist {
    pub async fn create(
        conn: &mut SqlitePoolConn,
        name: &str,
        folder_id: Option<RowId>,
    ) -> Result<RowId, sqlx::Error> {
        let id = sqlx::query(
            "INSERT INTO playlists (name, folder_id) VALUES (?, ?)",
        )
        .bind(name)
        .bind(folder_id)
        .execute(conn)
        .await?
        .last_insert_rowid();

        Ok(id)
    }

    pub async fn add_to_folder(
        conn: &mut SqlitePoolConn,
        playlist_id: RowId,
        folder_id: RowId,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE playlists SET folder_id = ? WHERE id = ?")
            .bind(folder_id)
            .bind(playlist_id)
            .execute(conn)
            .await
            .map(|_done| ())
    }

    pub async fn add_track(
        conn: &mut SqlitePoolConn,
        playlist_id: RowId,
        track_id: RowId,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO playlist_tracks (playlist_id, track_id) VALUES (?, ?)",
        )
        .bind(playlist_id)
        .bind(track_id)
        .execute(conn)
        .await
        .map(|_done| ())
    }
}

pub async fn import_parse_result(
    conn: SqlitePoolConn,
    collection_id: RowId,
    metadata: parse::ParseResult,
) -> OwnedTrack {
    let mut conn = conn;
    let mut artists = vec![];
    for curr_artist in metadata.artists {
        let new_artist = match Artist::create(&mut conn, &curr_artist).await {
            Ok(artist_id) => Artist::get(&mut conn, artist_id).await.unwrap(),
            Err(sqlx::Error::Database(d)) => match d.code() {
                Some(code) if code == SQLITE_UNIQUE_VIOLATION => {
                    Artist::find_by_name(&mut conn, &curr_artist).await.unwrap()
                }
                _ => panic!("new artist failed db {:?}", d),
            },
            Err(e) => panic!("new artist failed {:?}", e),
        };
        artists.push(new_artist);
    }

    let primary_artist = &artists[0];

    // TODO should this be wrapped around all release creation?
    let releases =
        ReleaseBase::get_artist_releases(&mut conn, primary_artist.id)
            .await
            .unwrap();

    let album = metadata.album.as_str();
    let artist_ids: Vec<_> = artists.iter().map(|a| a.id).collect();
    let release = match releases.iter().find(|r| r.name == album) {
        Some(r) => r.clone(),
        None => {
            let release_id = ReleaseBase::create(
                &mut conn,
                album,
                metadata.date.as_deref(),
                &artist_ids,
            )
            .await
            .unwrap();

            ReleaseBase::get(&mut conn, release_id).await.unwrap()
        }
    };

    let track_id = match TrackBase::create(
        &mut conn,
        &metadata.track,
        release.id,
        collection_id,
        metadata.path.to_str().unwrap(), // TODO
        metadata.channels as RowId,
        metadata.sample_rate as RowId,
        metadata.bit_depth as RowId,
        metadata.track_pos.map(|pos| pos as RowId),
    )
    .await
    {
        Ok(track_id) => track_id,
        Err(sqlx::Error::Database(e)) => match e.code() {
            Some(code) if code == SQLITE_UNIQUE_VIOLATION => {
                panic!("track with same data already exists {:?}", e)
            }
            Some(code) => panic!("track insert db failure {:?}", code),
            None => panic!("track insert db fail no code"),
        },
        Err(e) => panic!("track insert failed {:?}", e),
    };

    // FIXME don't need to return this
    let t = TrackBase::get(&mut conn, track_id)
        .await
        .expect("Failed to get track after insert");

    OwnedTrack {
        id: t.id,
        name: t.name,
        release: release.complete_owned(artists),
        tags: Vec::new(),
        file_path: t.file_path,
        channels: t.channels,
        sample_rate: t.sample_rate,
        bit_depth: t.bit_depth,
        track_num: t.track_num,
        created: t.created,
        modified: t.modified,
    }
}
