// use chrono::{DateTime, Utc};
use super::parse;
use core::borrow::BorrowMut;
use sqlx::{pool::PoolConnection, sqlite::Sqlite};
use std::collections::HashMap;

pub type SqlitePoolConn = PoolConnection<Sqlite>;
// type Timestamptz = DateTime<Utc>; // TODO figure out string conversion
const SQLITE_UNIQUE_VIOLATION: &'static str = "2067";

// sqlite doesn't have precise integers

// FIXME
// - handle encoding/decoding string array (for Track#tags)
// - handle datetime columns in structs to map to strings n onw

#[derive(Clone, Debug)]
pub struct Artist {
    pub id: i64,
    pub name: String,
    pub created: String, // TODO parse date
}

// TODO organize these fns
// in the sqlx examples, they impl organized traits on SqliteConnection
// which makes sense. but traits are more runtime work, right?

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
        release_id: i64,
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
    pub id: i64,
    pub name: String,
    pub date: Option<String>,
    pub created: String, // TODO parse date
}

impl Release {
    pub async fn create(
        conn: &mut SqlitePoolConn,
        name: &str,
        date: Option<&str>,
        artist_ids: Vec<i64>,
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
        artist_id: i64,
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
    pub artist_id: i64,
    pub release_id: i64,
}

impl ArtistRelease {
    async fn create(
        conn: &mut SqlitePoolConn,
        artist_id: i64,
        release_id: i64,
    ) -> Result<Self, sqlx::Error> {
        let id = sqlx::query(
            "INSERT INTO artist_releases (artist_id, release_id)
            VALUES (?, ?);",
        )
        .bind(artist_id)
        .bind(release_id)
        .execute(conn)
        .await?
        .last_insert_rowid();

        Ok(ArtistRelease {
            artist_id,
            release_id,
        })
    }
}

// TODO store track duration
#[derive(Clone, Debug)]
pub struct Track {
    pub id: i64,
    pub name: String,
    pub release_id: i64,
    pub file_path: String,
    pub channels: i64,
    pub sample_rate: i64,
    pub bit_depth: i64,
    pub track_num: Option<i64>,
    pub tags: String,     // TODO parse to array
    pub created: String,  // TODO parse date
    pub modified: String, // TODO parse date
}

#[derive(Debug)]
pub struct DetailedTrack {
    pub id: i64,
    pub name: String,
    // is RC worth it here? we have to clone release + artist names
    // for the UI either way.
    pub release: Release,
    pub artists: Vec<Artist>,
    pub file_path: String,
    pub channels: i64,
    pub sample_rate: i64,
    pub bit_depth: i64,
    pub track_num: Option<i64>,
    pub tags: String,     // TODO parse to array
    pub created: String,  // TODO parse date
    pub modified: String, // TODO parse date
}

impl Track {
    pub async fn create(
        conn: &mut SqlitePoolConn,
        name: &str,
        release_id: i64,
        file_path: &str,
        channels: i64,
        sample_rate: i64,
        bit_depth: i64,
        track_num: Option<i64>,
    ) -> Result<i64, sqlx::Error> {
        let track_id = sqlx::query(
            "INSERT INTO tracks
            (name, release_id, file_path, channels, sample_rate, bit_depth, track_num)
            VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(name)
        .bind(release_id)
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
        id: i64,
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
        // let tracks = Track::get_all(conn).await?;/
        // FIXME artists need to be selected separately. how in one query?
        let tracks_with_releases = sqlx::query!(
            "SELECT
                tracks.id,
                tracks.name,
                tracks.file_path,
                tracks.channels,
                tracks.sample_rate,
                tracks.bit_depth,
                tracks.track_num,
                tracks.tags,
                tracks.created,
                tracks.modified,
                releases.id as release_id,
                releases.name as release_name,
                releases.date as release_date,
                releases.created as release_created
            FROM tracks
            JOIN releases
            ON tracks.release_id = releases.id;",
        )
        .fetch_all(conn.borrow_mut())
        .await?;

        let mut release_artists = HashMap::new();
        // put into a map of release_id -> Vec<Artist>
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
        .iter()
        .for_each(|row| {
            let ra =
                release_artists.entry(row.release_id).or_insert(Vec::new());
            ra.push(Artist {
                id: row.id,
                name: row.name.clone(),
                created: row.created.clone(),
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
                file_path: track.file_path,
                channels: track.channels,
                sample_rate: track.sample_rate,
                bit_depth: track.bit_depth,
                track_num: track.track_num,
                tags: track.tags,
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

    // FIXME - is this really the case? there is a primary key constraint, verify
    // this is done to prevent duplicate entries
    // TODO should this just be wrapped up into the create release fn?
    let releases = Release::get_artist_releases(&mut conn, artists[0].id)
        .await
        .unwrap();

    let album = metadata.album.as_str();
    let release = match releases.iter().find(|r| r.name == album) {
        Some(r) => r.clone(), // FIXME avoid clone
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
        metadata.channels as i64,
        metadata.sample_rate as i64,
        metadata.bit_depth as i64,
        metadata.track_pos.map(|pos| pos as i64),
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
        file_path: t.file_path,
        channels: t.channels,
        sample_rate: t.sample_rate,
        bit_depth: t.bit_depth,
        track_num: t.track_num,
        tags: t.tags,
        created: t.created,
        modified: t.modified,
    }
}
