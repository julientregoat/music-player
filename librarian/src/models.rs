// use chrono::{DateTime, Utc};
use core::borrow::BorrowMut;
use sqlx::{pool::PoolConnection, sqlite::Sqlite};

pub type SqlitePoolConn = PoolConnection<Sqlite>;
// type Timestamptz = DateTime<Utc>; // TODO figure out string conversion

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

    pub async fn get(
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
    // hacky way to deal issue using mutable reference twice
    // TODO something better - maybe just don't have this logic in the same
    // fn, use a different publicly exposed wrapper?
    pub async fn create(
        conn: SqlitePoolConn,
        name: &str,
        date: Option<&str>,
        artist_ids: Vec<i64>,
    ) -> Result<(SqlitePoolConn, Self), sqlx::Error> {
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

        let release = sqlx::query_as!(
            Self,
            "SELECT * FROM releases WHERE id = ?;",
            release_id
        )
        .fetch_one(conn.borrow_mut())
        .await?;

        for id in artist_ids {
            ArtistRelease::create(&mut conn, id, release_id).await?;
        }

        Ok((conn, release))
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

#[derive(Clone, Debug)]
pub struct Track {
    pub id: i64,
    pub name: String,
    pub release_id: i64,
    pub file_path: String,
    pub channels: i64,
    pub sample_rate: i64,
    pub bit_rate: i64,
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
        bit_rate: i64,
        track_num: Option<i64>,
    ) -> Result<Self, sqlx::Error> {
        // Err(sqlx::Error::ColumnIndexOutOfBounds{len: 1, index: 1})
        let id = sqlx::query(
            "INSERT INTO tracks
            (name, release_id, file_path, channels, sample_rate, bit_rate, track_num)
            VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(name)
        .bind(release_id)
        .bind(file_path)
        .bind(channels)
        .bind(sample_rate)
        .bind(bit_rate)
        .bind(track_num)
        .execute(conn.borrow_mut())
        .await?
        .last_insert_rowid();

        sqlx::query_as!(Self, "SELECT * FROM tracks WHERE id = ?", id)
            .fetch_one(conn.borrow_mut())
            .await
    }
}
