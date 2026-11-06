use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Attachment {
    pub id: String,
    pub filename: String,
    pub size: u64,
    pub mime: String,
    pub kind: String,
    pub url: String,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Post {
    pub id: String,
    pub created_at: i64,
    pub body: Option<String>,
    pub attachments: Vec<Attachment>,
}

#[derive(Clone, Debug)]
pub struct NewAttachment {
    pub id: String,
    pub filename: String,
    pub size: u64,
    pub mime: String,
    pub kind: String,
}

pub struct Db(Mutex<Connection>);

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS posts (
  id         TEXT PRIMARY KEY,
  created_at INTEGER NOT NULL,
  body       TEXT
);
CREATE TABLE IF NOT EXISTS attachments (
  id        TEXT PRIMARY KEY,
  post_id   TEXT NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
  filename  TEXT NOT NULL,
  size      INTEGER NOT NULL,
  mime      TEXT NOT NULL,
  kind      TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS attachments_post_id ON attachments(post_id);
";

fn err(e: rusqlite::Error) -> String {
    format!("DB 오류: {e}")
}

pub fn attachment_url(id: &str, filename: &str) -> String {
    format!("/files/{id}/{}", crate::files::percent_encode(filename))
}

impl Db {
    fn init(conn: Connection) -> Result<Db, String> {
        conn.execute_batch("PRAGMA foreign_keys = ON;").map_err(err)?;
        conn.execute_batch(SCHEMA).map_err(err)?;
        Ok(Db(Mutex::new(conn)))
    }

    pub fn open(path: &Path) -> Result<Db, String> {
        let conn = Connection::open(path).map_err(err)?;
        conn.pragma_update(None, "journal_mode", "WAL").map_err(err)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Db, String> {
        Self::init(Connection::open_in_memory().map_err(err)?)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn insert_post(
        &self,
        id: &str,
        created_at: i64,
        body: Option<&str>,
        atts: &[NewAttachment],
    ) -> Result<Post, String> {
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(err)?;
        tx.execute(
            "INSERT INTO posts (id, created_at, body) VALUES (?1, ?2, ?3)",
            params![id, created_at, body],
        )
        .map_err(err)?;
        for a in atts {
            tx.execute(
                "INSERT INTO attachments (id, post_id, filename, size, mime, kind) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![a.id, id, a.filename, a.size as i64, a.mime, a.kind],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)?;
        Ok(Post {
            id: id.to_string(),
            created_at,
            body: body.map(str::to_string),
            attachments: atts
                .iter()
                .map(|a| Attachment {
                    id: a.id.clone(),
                    filename: a.filename.clone(),
                    size: a.size,
                    mime: a.mime.clone(),
                    kind: a.kind.clone(),
                    url: attachment_url(&a.id, &a.filename),
                })
                .collect(),
        })
    }

    fn attachments_for(conn: &Connection, post_id: &str) -> Result<Vec<Attachment>, String> {
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, filename, size, mime, kind FROM attachments WHERE post_id = ?1 ORDER BY id",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map(params![post_id], |r| {
                let id: String = r.get(0)?;
                let filename: String = r.get(1)?;
                Ok(Attachment {
                    url: attachment_url(&id, &filename),
                    id,
                    filename,
                    size: r.get::<_, i64>(2)? as u64,
                    mime: r.get(3)?,
                    kind: r.get(4)?,
                })
            })
            .map_err(err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(err)
    }

    pub fn list_posts(
        &self,
        before: Option<&str>,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Post>, String> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, created_at, body FROM posts
                 WHERE (?1 IS NULL OR id < ?1) AND (?2 IS NULL OR id > ?2)
                 ORDER BY id DESC LIMIT ?3",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map(params![before, after, limit as i64], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<String>>(2)?))
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        let mut posts = Vec::with_capacity(rows.len());
        for (id, created_at, body) in rows {
            let attachments = Self::attachments_for(&conn, &id)?;
            posts.push(Post { id, created_at, body, attachments });
        }
        Ok(posts)
    }

    pub fn get_post(&self, id: &str) -> Result<Option<Post>, String> {
        let conn = self.lock();
        let row = conn
            .query_row(
                "SELECT id, created_at, body FROM posts WHERE id = ?1",
                params![id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<String>>(2)?)),
            )
            .optional()
            .map_err(err)?;
        match row {
            None => Ok(None),
            Some((id, created_at, body)) => {
                let attachments = Self::attachments_for(&conn, &id)?;
                Ok(Some(Post { id, created_at, body, attachments }))
            }
        }
    }

    pub fn get_attachment(&self, id: &str) -> Result<Option<(String, Attachment)>, String> {
        let conn = self.lock();
        conn.query_row(
            "SELECT post_id, id, filename, size, mime, kind FROM attachments WHERE id = ?1",
            params![id],
            |r| {
                let post_id: String = r.get(0)?;
                let id: String = r.get(1)?;
                let filename: String = r.get(2)?;
                Ok((
                    post_id,
                    Attachment {
                        url: attachment_url(&id, &filename),
                        id,
                        filename,
                        size: r.get::<_, i64>(3)? as u64,
                        mime: r.get(4)?,
                        kind: r.get(5)?,
                    },
                ))
            },
        )
        .optional()
        .map_err(err)
    }

    pub fn delete_post(&self, id: &str) -> Result<bool, String> {
        let conn = self.lock();
        let n = conn
            .execute("DELETE FROM posts WHERE id = ?1", params![id])
            .map_err(err)?;
        Ok(n > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn att(id: &str, filename: &str, mime: &str, kind: &str) -> NewAttachment {
        NewAttachment {
            id: id.into(),
            filename: filename.into(),
            size: 3,
            mime: mime.into(),
            kind: kind.into(),
        }
    }

    #[test]
    fn insert_and_list_newest_first() {
        let db = Db::open_in_memory().unwrap();
        db.insert_post("01A", 1, Some("first"), &[]).unwrap();
        db.insert_post("01B", 2, None, &[att("A1", "x.png", "image/png", "image")])
            .unwrap();
        let posts = db.list_posts(None, None, 100).unwrap();
        assert_eq!(
            posts.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            ["01B", "01A"]
        );
        assert_eq!(posts[0].attachments[0].url, "/files/A1/x.png");
        assert_eq!(posts[0].attachments[0].size, 3);
        assert_eq!(posts[1].body.as_deref(), Some("first"));
        assert_eq!(db.list_posts(Some("01B"), None, 100).unwrap().len(), 1);
        assert_eq!(db.list_posts(None, Some("01A"), 100).unwrap()[0].id, "01B");
        assert_eq!(db.list_posts(None, None, 1).unwrap().len(), 1);
        assert_eq!(db.get_post("01B").unwrap().unwrap().attachments.len(), 1);
        assert!(db.get_post("nope").unwrap().is_none());
    }

    #[test]
    fn attachment_url_percent_encodes_filename() {
        assert_eq!(attachment_url("A1", "x.png"), "/files/A1/x.png");
        assert_eq!(attachment_url("A1", "#1 notes.txt"), "/files/A1/%231%20notes.txt");
        assert_eq!(attachment_url("A1", "한.txt"), "/files/A1/%ED%95%9C.txt");
    }

    #[test]
    fn delete_cascades_attachments() {
        let db = Db::open_in_memory().unwrap();
        db.insert_post(
            "01A",
            1,
            None,
            &[att("A1", "x", "application/octet-stream", "file")],
        )
        .unwrap();
        let (post_id, a) = db.get_attachment("A1").unwrap().unwrap();
        assert_eq!(post_id, "01A");
        assert_eq!(a.filename, "x");
        assert!(db.delete_post("01A").unwrap());
        assert!(db.get_attachment("A1").unwrap().is_none());
        assert!(!db.delete_post("01A").unwrap());
    }

    #[test]
    fn open_on_disk_creates_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        {
            let db = Db::open(&path).unwrap();
            db.insert_post("01A", 1, Some("x"), &[]).unwrap();
        }
        let db = Db::open(&path).unwrap();
        assert_eq!(db.list_posts(None, None, 10).unwrap().len(), 1);
    }
}
