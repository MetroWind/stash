use chrono::prelude::*;
use rusqlite as sql;
use rusqlite::OptionalExtension;
use serde::ser::{Serialize, SerializeStruct, Serializer};

use crate::error;
use crate::error::Error as Error;

#[derive(serde::Serialize)]
pub struct User
{
    pub id: i64,
    pub name: String,
}

impl User
{
    pub fn new(id: i64, name: String) -> User
    {
        Self{ id: id, name: name, }
    }
}

pub enum EntryOrder
{
    NewFirst,
    OldFirst,
}

pub struct Entry
{
    pub uri: String,
    pub title: String,
    pub time_add: chrono::DateTime<Utc>,
}

impl Serialize for Entry
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer,
    {
        let mut s = serializer.serialize_struct("Entry", 3)?;
        s.serialize_field("uri", &self.uri)?;
        s.serialize_field("title", &self.title)?;
        s.serialize_field(
            "time_add",
            &self.time_add.naive_local().format("%F %R").to_string())?;
        s.end()
    }
}

#[allow(dead_code)]
pub struct ReadEntry
{
    pub uri: String,
    pub title: String,
    pub time_add: chrono::DateTime<Utc>,
    pub time_read: chrono::DateTime<Utc>,
}

#[allow(dead_code)]
#[derive(Clone)]
pub enum SqliteFilename { InMemory, File(std::path::PathBuf) }

#[derive(Clone)]
pub struct DataManager
{
    filename: SqliteFilename,
    connection: Option<r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>>,
}

impl DataManager
{
    #[allow(dead_code)]
    pub fn new(f: SqliteFilename) -> Self
    {
        Self { filename: f, connection: None }
    }

    pub fn newWithFilename(f: &str) -> Self
    {
        Self {
            filename: SqliteFilename::File(std::path::PathBuf::from(f)),
            connection: None
        }
    }

    fn confirmConnection(&self) -> Result<r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>, Error>
    {
        if let Some(pool) = &self.connection
        {
            pool.get().map_err(|e| rterr!("Failed to get connection: {}", e))
        }
        else
        {
            Err(error!(DataError, "Sqlite database not connected"))
        }
    }

    /// Connect to the database. Create database file if not exist.
    pub fn connect(&mut self) -> Result<(), Error>
    {
        let manager = match &self.filename
        {
            SqliteFilename::File(path) =>
                r2d2_sqlite::SqliteConnectionManager::file(path),
            SqliteFilename::InMemory =>
                r2d2_sqlite::SqliteConnectionManager::memory(),
        };
        self.connection = Some(r2d2::Pool::new(manager).map_err(
            |_| rterr!("Failed to create connection pool"))?);
        Ok(())
    }

    fn tableExists(&self, table: &str) -> Result<bool, Error>
    {
        let conn = self.confirmConnection()?;
        let row = conn.query_row(
            "SELECT name FROM sqlite_master WHERE type='table' AND name=?;",
            sql::params![table],
            |row: &sql::Row|->sql::Result<String> { row.get(0) })
            .optional().map_err(
                |_| error!(DataError, "Failed to look up table {}", table))?;
        Ok(row.is_some())
    }

    pub fn init(&self) -> Result<(), Error>
    {
        let conn = self.confirmConnection()?;
        if !self.tableExists("user")?
        {
            conn.execute(
                "CREATE TABLE user (
                  id INTEGER PRIMARY KEY ASC,
                  name TEXT UNIQUE,
                  email TEXT UNIQUE,
                  key TEXT
                  );", []).map_err(
                |e| error!(DataError, "Failed to create table: {}", e))?;
        }
        Ok(())
    }

    pub fn findUser(&self, name: &str) -> Result<Option<User>, Error>
    {
        let conn = self.confirmConnection()?;
        conn.query_row(
            "SELECT id, name FROM user WHERE name=?;",
            sql::params![name],
            |row: &sql::Row|->sql::Result<User> {
                Ok(User::new(row.get(0)?, row.get(1)?))
            }).optional().map_err(
            |_| error!(DataError, "Failed to look up user {}", name))
    }

    pub fn createUser(&self, mut user: User) -> Result<User, Error>
    {
        let conn = self.confirmConnection()?;
        let row_count = conn.execute(
            "INSERT INTO user (name) VALUES (?);", sql::params![&user.name])
            .map_err(|e| error!(DataError, "Failed to create user: {}", e))?;
        if row_count != 1
        {
            return Err(error!(DataError, "Invalid insert happened"));
        }
        user.id = conn.last_insert_rowid();
        conn.execute(
            &format!("CREATE TABLE stash_{} (
                  uri TEXT UNIQUE,
                  title TEXT,
                  time_add INTEGER
                  )", user.id), []).map_err(
            |e| error!(DataError, "Failed to create table: {}", e))?;
        conn.execute(
            &format!("CREATE TABLE archive_{} (
                  uri TEXT UNIQUE,
                  title TEXT,
                  time_add INTEGER,
                  time_read INTEGER
                  )", user.id), []).map_err(
            |e| error!(DataError, "Failed to create table: {}", e))?;

        Ok(user)
    }

    pub fn addEntry(&self, user: &User, item: Entry) -> Result<(), Error>
    {
        let conn = self.confirmConnection()?;
        let time = item.time_add.timestamp();
        conn.execute(
            &format!("INSERT INTO stash_{} (uri, title, time_add)
                     VALUES (?, ?, ?) ON CONFLICT(uri) DO
                     UPDATE SET time_add={};", user.id, time),
            sql::params![&item.uri, &item.title, time])
            .map_err(|_| error!(DataError, "Failed to insert new entry"))?;
        Ok(())
    }

    pub fn findEntryByURI(&self, user: &User, uri: &str) ->
        Result<Option<Entry>, Error>
    {
        let conn = self.confirmConnection()?;
        conn.query_row(
            &format!("SELECT uri, title, time_add FROM stash_{} WHERE uri=?;",
                     user.id),
            sql::params![uri],
            |row: &sql::Row|->sql::Result<Entry> {
                let time_value = row.get(2)?;
                let time = if let chrono::LocalResult::Single(t) =
                    Utc.timestamp_opt(time_value, 0)
                {
                    t
                }
                else
                {
                    return Err(sql::Error::IntegralValueOutOfRange(
                        2, time_value));
                };
                Ok(Entry {
                    uri: row.get(0)?,
                    title: row.get(1)?,
                    time_add: time,
                })
            }).optional().map_err(
            |e| error!(DataError, "Failed to look up entry @ {}: {}", uri, e))
    }

    pub fn readEntry(&self, user: &User, item: &Entry) -> Result<(), Error>
    {
        let conn = self.confirmConnection()?;
        let time = item.time_add.timestamp();
        let now = Utc::now().timestamp();
        conn.execute(
            &format!("INSERT INTO archive_{} (uri, title, time_add, time_read)
                     VALUES (?, ?, ?, ?) ON CONFLICT(uri) DO
                     UPDATE SET time_add=?, time_read=?;", user.id),
            sql::params![item.uri.as_str(), &item.title, time, now, time, now])
            .map_err(|_| error!(DataError, "Failed to insert read entry"))?;
        conn.execute(
            &format!("DELETE FROM stash_{} WHERE uri=?;", user.id),
            sql::params![&item.uri])
            .map_err(|_| error!(DataError, "Failed to delete entry"))?;
        Ok(())
    }

    /// Retrieve “count” number of latest entries, starting from the
    /// entry at index “start_index”. Index starts at 0. Returned
    /// entries are sorted from new to old.
    pub fn getEntries(&self, user: &User, start_index: u64, count: u64,
                      order: EntryOrder) -> Result<Vec<Entry>, Error>
    {
        let conn = self.confirmConnection()?;

        let order_expr = match order
        {
            EntryOrder::NewFirst => "ORDER BY time_add DESC",
            EntryOrder::OldFirst => "ORDER BY time_add ASC",
        };

        let mut cmd = conn.prepare(
            &format!("SELECT uri, title, time_add FROM stash_{}
                     {} LIMIT ? OFFSET ?;", user.id, order_expr))
            .map_err(|e| error!(
                DataError,
                "Failed to compare statement to get entries: {}", e))?;
        let mut result = Vec::new();
        let rows = cmd.query_map([count, start_index], |row| {
            let time_value = row.get(2)?;
            let time = if let chrono::LocalResult::Single(t) =
                Utc.timestamp_opt(time_value, 0)
            {
                t
            }
            else
            {
                return Err(sql::Error::IntegralValueOutOfRange(
                    2, time_value));
            };
            Ok(Entry{
                uri: row.get(0)?,
                title: row.get(1)?,
                time_add: time,
            })
        }).map_err(|e| error!(DataError, "Failed to retrieve entries: {}", e))?;
        for row in rows
        {
            result.push(row.map_err(
                |e| error!(DataError, "Failed to retrieve row: {}", e))?);
        }
        Ok(result)
    }
}
