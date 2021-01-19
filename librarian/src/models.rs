// use chrono::{DateTime, Utc};
use super::parse;
use core::borrow::BorrowMut;
use sqlx::{pool::PoolConnection, sqlite::Sqlite};
use std::collections::HashMap;

pub type SqlitePoolConn = PoolConnection<Sqlite>;
pub type RowId = i64;
// type Timestamptz = DateTime<Utc>; // TODO figure out string conversion

const SQLITE_UNIQUE_VIOLATION: &'static str = "2067";

// TODO refactor exposed API? associated functions doesn't feel ideal
// sqlx examples show models organized as traits implemented on the Connection
// type. this is more ergonomic, but unneeded runtime work?
// separate higher level composed fns from base layer?
// TODO don't look up created tracks, just return last_insert_rowid

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
    ) -> Result<Self, sqlx::Error> {
        let id = sqlx::query("INSERT INTO artists (name) VALUES (?);")
            .bind(name)
            .execute(conn.borrow_mut())
            .await?
            .last_insert_rowid();

        sqlx::query_as!(
            Self,
            "SELECT id, name, created FROM artists WHERE id = ?;",
            id
        )
        .fetch_one(conn.borrow_mut())
        .await
    }

    pub async fn get_by_name(
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
pub struct Release {
    pub id: RowId,
    pub name: String,
    pub date: Option<String>,
    pub created: String, // TODO parse date
}

impl Release {
    pub async fn create(
        conn: &mut SqlitePoolConn,
        name: &str,
        date: Option<&str>,
        artist_ids: Vec<RowId>,
    ) -> Result<Self, sqlx::Error> {
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
            ArtistRelease::create(&mut conn, id, release_id).await?;
        }

        sqlx::query_as!(
            Self,
            "SELECT * FROM releases WHERE id = ?;",
            release_id
        )
        .fetch_one(conn.borrow_mut())
        .await
    }

    pub async fn get_by_name(
        conn: &mut SqlitePoolConn,
        name: &str,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(Self, "SELECT * FROM releases WHERE name = ?", name)
            .fetch_all(conn)
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
        tag_name: &str,
    ) -> Result<RowId, sqlx::Error> {
        let id = sqlx::query("INSERT INTO tags (name) VALUES (?)")
            .bind(tag_name)
            .execute(conn)
            .await?
            .last_insert_rowid();

        Ok(id)
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
pub struct Track {
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

// TODO how significant of an impact on memory does duplicating Artist, Release,
// and Tags per track have? performance?
// - use [A]RC to reference count and share memory
// - for Vec<DetailedTrack> etc,  wrap + store Artists/Releases/Tags and use
// refs in this struct
// - stack allocated str (e.g inlinable_string) for performance
#[derive(Debug)]
pub struct DetailedTrack {
    pub id: RowId,
    pub name: String,
    pub release: Release,
    pub artists: Vec<Artist>,
    pub tags: Vec<Tag>,
    pub file_path: String,
    pub channels: i64,
    pub sample_rate: i64,
    pub bit_depth: i64,
    pub track_num: Option<i64>,
    pub created: String,  // TODO parse date
    pub modified: String, // TODO parse date
}

impl Track {
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

    // TODO should this be outside the Track impl?
    pub async fn get_all_detailed(
        conn: &mut SqlitePoolConn,
    ) -> Result<Vec<DetailedTrack>, sqlx::Error> {
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
            let dt = DetailedTrack {
                id: track.id,
                name: track.name,
                release: Release {
                    id: track.release_id,
                    name: track.release_name,
                    date: track.release_date,
                    created: track.release_created,
                },
                artists: release_artists
                    .get(&track.release_id)
                    .unwrap()
                    .to_owned(),
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

pub async fn import_from_parse_result(
    conn: SqlitePoolConn,
    metadata: parse::ParseResult,
) -> DetailedTrack {
    let mut conn = conn;
    let mut artists = vec![];
    for curr_artist in metadata.artists {
        let new_artist = match Artist::create(&mut conn, &curr_artist).await {
            Ok(a) => a,
            Err(sqlx::Error::Database(d)) => match d.code() {
                Some(code) if code == SQLITE_UNIQUE_VIOLATION => {
                    Artist::get_by_name(&mut conn, &curr_artist).await.unwrap()
                }
                _ => panic!("new artist failed db {:?}", d),
            },
            Err(e) => panic!("new artist failed {:?}", e),
        };
        artists.push(new_artist);
    }

    let primary_artist = &artists[0];

    // TODO should this be wrapped around all release creation?
    let releases = Release::get_artist_releases(&mut conn, primary_artist.id)
        .await
        .unwrap();

    let album = metadata.album.as_str();
    let release = match releases.iter().find(|r| r.name == album) {
        Some(r) => r.clone(),
        None => Release::create(
            &mut conn,
            album,
            metadata.date.as_deref(),
            artists.iter().map(|a| a.id).collect(),
        )
        .await
        .unwrap(),
    };

    let track_id = match Track::create(
        &mut conn,
        &metadata.track,
        release.id,
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
    let t = Track::get(&mut conn, track_id)
        .await
        .expect("Failed to get track after insert");

    DetailedTrack {
        id: t.id,
        name: t.name,
        release,
        artists,
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
