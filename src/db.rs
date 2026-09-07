use crate::models::{AspectFilter, ImageRecord, SortMode};
use crate::natural_sort::compare_natural;
use rand::seq::SliceRandom;
use rusqlite::{params, Connection, Result};
use std::path::{Path, PathBuf};

pub struct Database {
    pub db_path: PathBuf,
}

#[derive(Default, Clone)]
pub struct QueryFilter {
    pub root_dir: String,
    pub subfolder: Option<String>,
    pub color_name: Option<String>,
    pub aspect_type: AspectFilter,
    pub search_term: String,
    pub favorite_only: bool,
    pub sort_by: SortMode,
}

#[allow(dead_code)]
pub struct Stats {
    pub total_images: usize,
    pub total_bytes: u64,
    pub favorites_count: usize,
    pub indexed_count: usize,
}

impl Database {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let db_path = path.as_ref().to_path_buf();
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let db = Self { db_path };
        let _ = db.init_schema();
        db
    }

    pub fn default_db() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let path = PathBuf::from(&home)
            .join(".cache")
            .join("omniview")
            .join("omniview_index.db");
        let legacy_path = PathBuf::from(&home)
            .join(".cache")
            .join("pixiv_gallery")
            .join("pixiv_index.db");
        if !path.exists() && legacy_path.exists() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::copy(&legacy_path, &path);
        }
        Self::new(path)
    }

    pub fn get_connection(&self) -> Result<Connection> {
        let conn = Connection::open(&self.db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA cache_size = -32000;",
        )?;
        Ok(conn)
    }

    pub fn init_schema(&self) -> Result<()> {
        let conn = self.get_connection()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS images (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT UNIQUE,
                filename TEXT,
                subfolder TEXT,
                file_size INTEGER,
                mtime REAL,
                ctime REAL DEFAULT 0,
                width INTEGER DEFAULT 0,
                height INTEGER DEFAULT 0,
                aspect_type TEXT DEFAULT '',
                color_name TEXT DEFAULT '',
                color_r INTEGER DEFAULT 0,
                color_g INTEGER DEFAULT 0,
                color_b INTEGER DEFAULT 0,
                dhash TEXT DEFAULT '',
                thumb_path TEXT DEFAULT '',
                is_favorite INTEGER DEFAULT 0,
                indexed INTEGER DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_path ON images(path);
            CREATE INDEX IF NOT EXISTS idx_subfolder ON images(subfolder);
            CREATE INDEX IF NOT EXISTS idx_color ON images(color_name);
            CREATE INDEX IF NOT EXISTS idx_aspect ON images(aspect_type);
            CREATE INDEX IF NOT EXISTS idx_mtime ON images(mtime);
            CREATE INDEX IF NOT EXISTS idx_ctime ON images(ctime);
            CREATE INDEX IF NOT EXISTS idx_favorite ON images(is_favorite);",
        )?;
        Ok(())
    }

    pub fn batch_sync_files(&self, entries: &[ImageRecord]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        let mut conn = self.get_connection()?;
        let tx = conn.transaction()?;

        {
            let mut stmt = tx.prepare(
                "INSERT INTO images (path, filename, subfolder, file_size, mtime, ctime, indexed)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)
                 ON CONFLICT(path) DO UPDATE SET
                     file_size = excluded.file_size,
                     mtime = excluded.mtime,
                     ctime = excluded.ctime
                 WHERE images.mtime != excluded.mtime OR images.ctime != excluded.ctime;",
            )?;

            for r in entries {
                let path_str = r.path.to_string_lossy().to_string();
                stmt.execute(params![
                    path_str,
                    r.filename,
                    r.subfolder,
                    r.file_size,
                    r.mtime,
                    r.ctime
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    pub fn toggle_favorite(&self, path: &str) -> Result<bool> {
        let conn = self.get_connection()?;
        let current_fav: i32 = conn.query_row(
            "SELECT is_favorite FROM images WHERE path = ?1;",
            params![path],
            |row| row.get(0),
        ).unwrap_or(0);

        let new_fav = if current_fav == 1 { 0 } else { 1 };
        conn.execute(
            "UPDATE images SET is_favorite = ?1 WHERE path = ?2;",
            params![new_fav, path],
        )?;

        Ok(new_fav == 1)
    }

    pub fn update_metadata(
        &self,
        path: &str,
        width: u32,
        height: u32,
        aspect_type: &str,
        color_name: &str,
        r: u8,
        g: u8,
        b: u8,
        dhash: &str,
        thumb_path: &str,
    ) -> Result<()> {
        let conn = self.get_connection()?;
        conn.execute(
            "UPDATE images SET
                width = ?1, height = ?2, aspect_type = ?3,
                color_name = ?4, color_r = ?5, color_g = ?6, color_b = ?7,
                dhash = ?8, thumb_path = ?9, indexed = 1
             WHERE path = ?10;",
            params![
                width, height, aspect_type, color_name, r, g, b, dhash, thumb_path, path
            ],
        )?;
        Ok(())
    }

    pub fn get_unindexed_paths(&self, root_dir: &str) -> Result<Vec<(String, f64, u64)>> {
        let conn = self.get_connection()?;
        let pattern = format!("{}/%", root_dir.trim_end_matches('/'));
        let mut stmt = conn.prepare(
            "SELECT path, mtime, file_size FROM images 
             WHERE (path = ?1 OR path LIKE ?2) AND indexed = 0;",
        )?;
        let rows = stmt.query_map(params![root_dir, pattern], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        let mut results = Vec::new();
        for r in rows {
            if let Ok(item) = r {
                results.push(item);
            }
        }
        Ok(results)
    }

    #[allow(dead_code)]
    pub fn get_subfolders_with_counts(&self, root_dir: &str) -> Result<Vec<(String, usize)>> {
        let conn = self.get_connection()?;
        let pattern = format!("{}/%", root_dir.trim_end_matches('/'));
        let mut stmt = conn.prepare(
            "SELECT subfolder, COUNT(*) as cnt
             FROM images
             WHERE path LIKE ?1
             GROUP BY subfolder
             ORDER BY subfolder ASC;",
        )?;

        let mut rows = stmt.query(params![pattern])?;
        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            let sub: String = row.get(0)?;
            let cnt: usize = row.get(1)?;
            result.push((sub, cnt));
        }

        Ok(result)
    }

    pub fn get_stats(&self) -> Result<Stats> {
        let conn = self.get_connection()?;
        let (total_images, total_bytes): (usize, Option<u64>) = conn.query_row(
            "SELECT COUNT(*), SUM(file_size) FROM images;",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap_or((0, None));

        let favorites_count: usize = conn.query_row(
            "SELECT COUNT(*) FROM images WHERE is_favorite = 1;",
            [],
            |row| row.get(0),
        ).unwrap_or(0);

        let indexed_count: usize = conn.query_row(
            "SELECT COUNT(*) FROM images WHERE indexed = 1;",
            [],
            |row| row.get(0),
        ).unwrap_or(0);

        Ok(Stats {
            total_images,
            total_bytes: total_bytes.unwrap_or(0),
            favorites_count,
            indexed_count,
        })
    }

    pub fn query_images(&self, filter: &QueryFilter) -> Result<Vec<ImageRecord>> {
        let conn = self.get_connection()?;
        let mut clauses = Vec::new();
        let mut params_vec: Vec<rusqlite::types::Value> = Vec::new();

        if !filter.root_dir.is_empty() {
            let prefix = format!("{}/%", filter.root_dir.trim_end_matches('/'));
            clauses.push("(path = ? OR path LIKE ?)");
            params_vec.push(rusqlite::types::Value::Text(filter.root_dir.clone()));
            params_vec.push(rusqlite::types::Value::Text(prefix));
        }

        if let Some(ref sub) = filter.subfolder {
            if sub != "__all__" {
                if sub == "__root__" || sub.is_empty() {
                    clauses.push("subfolder = ''");
                } else {
                    clauses.push("subfolder = ?");
                    params_vec.push(rusqlite::types::Value::Text(sub.clone()));
                }
            }
        }

        if let Some(ref col) = filter.color_name {
            if col != "all" {
                clauses.push("color_name = ?");
                params_vec.push(rusqlite::types::Value::Text(col.clone()));
            }
        }

        match filter.aspect_type {
            AspectFilter::Portrait => {
                clauses.push("aspect_type = 'portrait'");
            }
            AspectFilter::Landscape => {
                clauses.push("aspect_type = 'landscape'");
            }
            AspectFilter::Square => {
                clauses.push("aspect_type = 'square'");
            }
            AspectFilter::All => {}
        }

        if filter.favorite_only {
            clauses.push("is_favorite = 1");
        }

        if !filter.search_term.trim().is_empty() {
            let pat = format!("%{}%", filter.search_term.trim());
            clauses.push("(filename LIKE ? OR subfolder LIKE ?)");
            params_vec.push(rusqlite::types::Value::Text(pat.clone()));
            params_vec.push(rusqlite::types::Value::Text(pat));
        }

        let where_clause = if clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", clauses.join(" AND "))
        };

        let sort_sql = match filter.sort_by {
            SortMode::ModifiedNewest => "mtime DESC",
            SortMode::ModifiedOldest => "mtime ASC",
            SortMode::CreatedNewest => "ctime DESC",
            SortMode::CreatedOldest => "ctime ASC",
            SortMode::SizeLargest => "file_size DESC",
            SortMode::SizeSmallest => "file_size ASC",
            _ => "mtime DESC",
        };

        let sql = format!(
            "SELECT path, filename, subfolder, file_size, mtime, ctime,
                    width, height, aspect_type, color_name,
                    color_r, color_g, color_b, dhash, thumb_path, is_favorite, indexed
             FROM images
             {}
             ORDER BY {};",
            where_clause, sort_sql
        );

        let mut stmt = conn.prepare(&sql)?;
        let rusqlite_params: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();

        let mut rows = stmt.query(rusqlite_params.as_slice())?;
        let mut results = Vec::new();

        while let Some(row) = rows.next()? {
            let path_str: String = row.get(0)?;
            let filename: String = row.get(1)?;
            let subfolder: String = row.get(2)?;
            let file_size: u64 = row.get(3)?;
            let mtime: f64 = row.get(4)?;
            let ctime: f64 = row.get(5)?;
            let width: u32 = row.get(6)?;
            let height: u32 = row.get(7)?;
            let aspect_str: String = row.get(8)?;
            let color_name: String = row.get(9)?;
            let _color_r: u8 = row.get(10)?;
            let _color_g: u8 = row.get(11)?;
            let _color_b: u8 = row.get(12)?;
            let dhash: String = row.get(13)?;
            let thumb_path_str: String = row.get(14)?;
            let is_favorite: bool = row.get::<_, i32>(15)? == 1;

            let aspect = match aspect_str.as_str() {
                "portrait" => AspectFilter::Portrait,
                "landscape" => AspectFilter::Landscape,
                _ => AspectFilter::Square,
            };

            results.push(ImageRecord {
                path: PathBuf::from(path_str),
                filename,
                subfolder,
                file_size,
                mtime,
                ctime,
                width,
                height,
                aspect,
                color_name,
                dhash,
                thumb_path: PathBuf::from(thumb_path_str),
                is_favorite,
            });
        }

        // Handle in-memory post-sorting for Natural sorting and Random shuffle
        match filter.sort_by {
            SortMode::NameAsc => {
                results.sort_by(|a, b| compare_natural(&a.filename, &b.filename));
            }
            SortMode::NameDesc => {
                results.sort_by(|a, b| compare_natural(&b.filename, &a.filename));
            }
            SortMode::Random => {
                let mut rng = rand::thread_rng();
                results.shuffle(&mut rng);
            }
            _ => {}
        }

        Ok(results)
    }
}

impl QueryFilter {
    #[allow(dead_code)]
    pub fn favorite_only(&self) -> bool {
        self.favorite_only
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_db_crud() {
        let temp_dir = std::env::temp_dir().join(format!("omniview_test_{}.db", std::process::id()));
        let db = Database::new(&temp_dir);

        let records = vec![
            ImageRecord {
                path: PathBuf::from("/test/img1.jpg"),
                filename: "img1.jpg".to_string(),
                subfolder: "folder_a".to_string(),
                file_size: 1024,
                mtime: 100.0,
                ctime: 100.0,
                width: 100,
                height: 100,
                aspect: AspectFilter::Square,
                color_name: "blue".to_string(),
                dhash: "0123456789abcdef".to_string(),
                thumb_path: PathBuf::new(),
                is_favorite: false,
            },
            ImageRecord {
                path: PathBuf::from("/test/img2.jpg"),
                filename: "img2.jpg".to_string(),
                subfolder: "folder_a".to_string(),
                file_size: 2048,
                mtime: 200.0,
                ctime: 200.0,
                width: 100,
                height: 200,
                aspect: AspectFilter::Portrait,
                color_name: "red".to_string(),
                dhash: "fedcba9876543210".to_string(),
                thumb_path: PathBuf::new(),
                is_favorite: true,
            },
        ];

        db.batch_sync_files(&records).expect("sync failed");

        let stats = db.get_stats().expect("stats failed");
        assert_eq!(stats.total_images, 2);

        // Test favorite toggle
        let new_fav = db.toggle_favorite("/test/img1.jpg").unwrap();
        assert!(new_fav);

        // Clean up
        let _ = std::fs::remove_file(&temp_dir);
    }
}
