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
    /// 자동 정리로 파일이 지워졌으면 true (글과 메타데이터는 남는다).
    #[serde(default)]
    pub purged: bool,
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
  kind      TEXT NOT NULL,
  purged    INTEGER NOT NULL DEFAULT 0
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
        Self::migrate(&conn)?;
        Ok(Db(Mutex::new(conn)))
    }

    /// 예전 DB 에 없는 컬럼을 추가한다.
    fn migrate(conn: &Connection) -> Result<(), String> {
        let has_purged = conn
            .prepare("PRAGMA table_info(attachments)")
            .map_err(err)?
            .query_map([], |r| r.get::<_, String>(1))
            .map_err(err)?
            .filter_map(Result::ok)
            .any(|c| c == "purged");
        if !has_purged {
            conn.execute_batch("ALTER TABLE attachments ADD COLUMN purged INTEGER NOT NULL DEFAULT 0")
                .map_err(err)?;
        }
        Ok(())
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
                    purged: false,
                })
                .collect(),
        })
    }

    fn attachments_for(conn: &Connection, post_id: &str) -> Result<Vec<Attachment>, String> {
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, filename, size, mime, kind, purged FROM attachments WHERE post_id = ?1 ORDER BY id",
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
                    purged: r.get::<_, i64>(5)? != 0,
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
            "SELECT post_id, id, filename, size, mime, kind, purged FROM attachments WHERE id = ?1",
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
                        purged: r.get::<_, i64>(6)? != 0,
                    },
                ))
            },
        )
        .optional()
        .map_err(err)
    }

    /// `cutoff_ms` 보다 오래된 글의 아직 안 지워진 첨부를 purged 로 표시하고,
    /// 영향을 받은 글들을 돌려준다. 파일 삭제는 호출자가 한다.
    pub fn purge_attachments_before(&self, cutoff_ms: i64) -> Result<Vec<Post>, String> {
        let ids: Vec<String> = {
            let conn = self.lock();
            let mut stmt = conn
                .prepare_cached(
                    "SELECT DISTINCT a.post_id FROM attachments a JOIN posts p ON p.id = a.post_id
                     WHERE p.created_at < ?1 AND a.purged = 0 ORDER BY a.post_id",
                )
                .map_err(err)?;
            let rows = stmt
                .query_map(params![cutoff_ms], |r| r.get::<_, String>(0))
                .map_err(err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(err)?;
            for id in &rows {
                conn.execute(
                    "UPDATE attachments SET purged = 1 WHERE post_id = ?1 AND purged = 0",
                    params![id],
                )
                .map_err(err)?;
            }
            rows
        };
        let mut posts = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(p) = self.get_post(&id)? {
                posts.push(p);
            }
        }
        Ok(posts)
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
    fn purge_marks_old_attachments_and_returns_affected_posts() {
        let db = Db::open_in_memory().unwrap();
        db.insert_post("01A", 1_000, Some("old text"), &[att("A1", "a.bin", "application/octet-stream", "file"), att("A2", "b.png", "image/png", "image")]).unwrap();
        db.insert_post("01B", 1_000, Some("old text only"), &[]).unwrap();
        db.insert_post("01C", 9_000, None, &[att("A3", "c.bin", "application/octet-stream", "file")]).unwrap();
        let affected = db.purge_attachments_before(5_000).unwrap();
        assert_eq!(affected.len(), 1);
        assert_eq!(affected[0].id, "01A");
        assert!(affected[0].attachments.iter().all(|a| a.purged));
        assert_eq!(affected[0].body.as_deref(), Some("old text"));
        // 두 번째 호출은 이미 정리된 것을 다시 돌려주지 않는다
        assert!(db.purge_attachments_before(5_000).unwrap().is_empty());
        let posts = db.list_posts(None, None, 10).unwrap();
        let c = posts.iter().find(|p| p.id == "01C").unwrap();
        assert!(!c.attachments[0].purged);
        let (_, a1) = db.get_attachment("A1").unwrap().unwrap();
        assert!(a1.purged);
    }

    #[test]
    fn open_migrates_schema_without_purged_column() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE posts (id TEXT PRIMARY KEY, created_at INTEGER NOT NULL, body TEXT);
                 CREATE TABLE attachments (id TEXT PRIMARY KEY, post_id TEXT NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
                   filename TEXT NOT NULL, size INTEGER NOT NULL, mime TEXT NOT NULL, kind TEXT NOT NULL);
                 INSERT INTO posts VALUES ('01A', 1, NULL);
                 INSERT INTO attachments VALUES ('A1', '01A', 'x', 1, 'text/plain', 'file');",
            )
            .unwrap();
        }
        let db = Db::open(&path).unwrap();
        let posts = db.list_posts(None, None, 10).unwrap();
        assert_eq!(posts[0].attachments[0].purged, false);
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
