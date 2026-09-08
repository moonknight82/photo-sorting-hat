mod export;
pub mod metadata;
pub mod recipe;
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use fs2::FileExt;
pub use recipe::Recipe;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::UNIX_EPOCH,
};
use unicode_normalization::UnicodeNormalization;
use walkdir::WalkDir;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Progress {
    pub phase: String,
    pub completed: u64,
    pub total: u64,
    pub current: String,
}
pub type Notify<'a> = &'a mut dyn FnMut(Progress);
type PlanRow = (
    i64,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    String,
);
fn progress(notify: &mut Notify<'_>, phase: &str, completed: u64, total: u64, current: &str) {
    notify(Progress {
        phase: phase.into(),
        completed,
        total,
        current: current.into(),
    });
}
pub fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        bail!("Cancelled; recorded progress is safe to resume");
    }
    Ok(())
}

pub struct Engine {
    pub(crate) db: Connection,
    db_path: PathBuf,
}
pub struct JobLock {
    _file: File,
}
impl Drop for JobLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self._file);
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Companion {
    pub path: String,
    pub extension: String,
    pub hash: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub id: i64,
    pub source: String,
    pub destination: String,
    pub status: String,
    pub keywords: Vec<String>,
    pub warning: Option<String>,
    pub error: Option<String>,
    pub hash: Option<String>,
    pub size: u64,
    pub kind: String,
    pub companions: Vec<Companion>,
    pub metadata: Value,
}
impl Engine {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(15))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
          CREATE TABLE IF NOT EXISTS config(key TEXT PRIMARY KEY,value TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS files(id INTEGER PRIMARY KEY,source TEXT NOT NULL UNIQUE,root TEXT NOT NULL,size INTEGER NOT NULL,mtime TEXT NOT NULL,metadata TEXT NOT NULL,folders TEXT NOT NULL,companions TEXT NOT NULL,hash TEXT,kind TEXT NOT NULL,error TEXT);
          CREATE INDEX IF NOT EXISTS files_size ON files(size); CREATE INDEX IF NOT EXISTS files_hash ON files(hash);
          CREATE TABLE IF NOT EXISTS items(id INTEGER PRIMARY KEY REFERENCES files(id),destination TEXT NOT NULL,keywords TEXT NOT NULL,status TEXT NOT NULL,warning TEXT,error TEXT,before_hash TEXT,after_hash TEXT);
          CREATE INDEX IF NOT EXISTS items_status ON items(status,id);
          CREATE TABLE IF NOT EXISTS reservations(path TEXT PRIMARY KEY,item INTEGER NOT NULL);
          CREATE TABLE IF NOT EXISTS collision_counters(path TEXT PRIMARY KEY,next INTEGER NOT NULL);
          CREATE TABLE IF NOT EXISTS artifacts(item INTEGER NOT NULL,path TEXT NOT NULL,stage TEXT NOT NULL,hash TEXT NOT NULL,identity TEXT NOT NULL,PRIMARY KEY(item,path));
          CREATE TABLE IF NOT EXISTS cleanup(item INTEGER NOT NULL,path TEXT NOT NULL,hash TEXT NOT NULL,done INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(item,path));
          CREATE TABLE IF NOT EXISTS scan_errors(path TEXT PRIMARY KEY,error TEXT NOT NULL);")?;
        let engine = Self {
            db,
            db_path: path.to_path_buf(),
        };
        if let Some(version) = engine.setting("schema_version")? {
            if version != "2" {
                bail!("Unsupported database schema {version}");
            }
        } else {
            engine.set("schema_version", "2")?;
        }
        Ok(engine)
    }
    pub fn lock(&self) -> Result<JobLock> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.db_path.with_extension("lock"))?;
        file.try_lock_exclusive()
            .context("Another job is using this archive index")?;
        Ok(JobLock { _file: file })
    }
    pub(crate) fn set(&self, key: &str, value: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO config VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row("SELECT value FROM config WHERE key=?1", [key], |r| r.get(0))
            .optional()?)
    }
    pub fn summary(&self) -> Result<Value> {
        let count: i64 = self
            .db
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
        let bytes: i64 = self
            .db
            .query_row("SELECT COALESCE(SUM(size),0) FROM files", [], |r| r.get(0))?;
        let mut statuses = serde_json::Map::new();
        let mut query = self
            .db
            .prepare("SELECT status,COUNT(*) FROM items GROUP BY status")?;
        for row in query.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
            let (key, value) = row?;
            statuses.insert(key, json!(value));
        }
        let errors: i64 = self
            .db
            .query_row("SELECT COUNT(*) FROM scan_errors", [], |r| r.get(0))?;
        Ok(
            json!({"files":count,"bytes":bytes,"statuses":statuses,"scan_errors":errors,"scan_state":self.setting("scan_state")?,"plan_state":self.setting("plan_state")?,"output":self.setting("output")?,"recipe":self.setting("recipe")?.and_then(|s|serde_json::from_str::<Value>(&s).ok())}),
        )
    }
    pub fn scan_errors(&self, after: &str) -> Result<Vec<Value>> {
        let mut q = self
            .db
            .prepare("SELECT path,error FROM scan_errors WHERE path>?1 ORDER BY path LIMIT 100")?;
        let rows = q.query_map([after], |r| {
            Ok(json!({"path":r.get::<_,String>(0)?,"error":r.get::<_,String>(1)?}))
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }
    pub fn folders(&self) -> Result<Vec<String>> {
        let mut q=self.db.prepare("SELECT DISTINCT j.value FROM files, json_each(files.folders) j ORDER BY j.value LIMIT 10000")?;
        let result: Vec<String> = q
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(result
            .into_iter()
            .filter(|name| recipe::is_meaningful_folder_name(name))
            .collect())
    }
    pub fn scan(
        &self,
        sources: &[PathBuf],
        output: &Path,
        cancel: &AtomicBool,
        mut notify: Notify<'_>,
    ) -> Result<()> {
        let _lock = self.lock()?;
        if sources.is_empty() {
            bail!("Select at least one source folder");
        }
        let output = absolute_directory(output)?;
        let roots: Vec<PathBuf> = sources
            .iter()
            .map(|p| fs::canonicalize(p).with_context(|| format!("Cannot open {}", p.display())))
            .collect::<Result<_>>()?;
        for root in &roots {
            if !root.is_dir() || root.starts_with(&output) {
                bail!("Source must be a directory outside the output folder");
            }
        }
        let roots_json = serde_json::to_string(&roots)?;
        // Completed exports keep their manifest. A fresh archive gets a new database.
        if self
            .db
            .query_row("SELECT COUNT(*) FROM items", [], |r| r.get::<_, i64>(0))?
            > 0
        {
            bail!("This index already has an export plan. Create a new session to scan another archive.");
        }
        if let Some(previous) = self.setting("roots")? {
            if previous != roots_json
                || self.setting("output")?.as_deref() != Some(&metadata::path_string(&output)?)
            {
                bail!("Resume requires the same sources and output; start a new session to change them");
            }
        }
        self.set("roots", &roots_json)?;
        self.set("output", &metadata::path_string(&output)?)?;
        self.set("scan_state", "scanning")?;
        let mut tool = metadata::MetadataTool::new()?;
        let supported = tool.extensions()?;
        let mut count = 0u64;
        let mut transaction = Some(self.db.unchecked_transaction()?);
        for root in &roots {
            for entry in WalkDir::new(root)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| {
                    !e.path().starts_with(&output) && e.file_name() != ".photo-hat-staging"
                })
            {
                check_cancel(cancel)?;
                let entry = match entry {
                    Ok(e) => e,
                    Err(e) => {
                        self.scan_error(e.path().unwrap_or(root), &e.to_string())?;
                        continue;
                    }
                };
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.path();
                if path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("._"))
                {
                    continue;
                }
                let ext = extension(path);
                if !metadata::is_photo(&ext) && !metadata::is_video(&ext) {
                    continue;
                }
                count += 1;
                if count % 25 == 1 {
                    progress(&mut notify, "scan", count, 0, &path.display().to_string());
                }
                let result = (|| -> Result<()> {
                    if !supported.contains(&ext) {
                        bail!("Format is not supported by bundled ExifTool");
                    }
                    let source = metadata::path_string(path)?;
                    let (size, mtime) = fingerprint(path)?;
                    let existing: Option<(u64, String)> = self
                        .db
                        .query_row(
                            "SELECT size,mtime FROM files WHERE source=?1",
                            [&source],
                            |r| Ok((r.get(0)?, r.get(1)?)),
                        )
                        .optional()?;
                    let companions = companions(path)?;
                    if let Some((old_size, old_time)) = existing {
                        let old: String = self.db.query_row(
                            "SELECT companions FROM files WHERE source=?1",
                            [&source],
                            |r| r.get(0),
                        )?;
                        if old_size == size
                            && old_time == mtime
                            && old == serde_json::to_string(&companions)?
                        {
                            return Ok(());
                        }
                    }
                    let mut meta = tool.read(path)?;
                    let detected = metadata::get(&meta, &["FileTypeExtension"])
                        .unwrap_or_default()
                        .to_lowercase();
                    if metadata::get(&meta, &["FileType"]).as_deref() == Some("MacOS")
                        || (!metadata::is_photo(&detected) && !metadata::is_video(&detected))
                    {
                        bail!("File contents are not recognized as supported photo/video media");
                    }
                    let mut tags: BTreeSet<String> =
                        metadata::keywords_from(&meta).into_iter().collect();
                    for c in &companions {
                        if c.extension == "xmp" {
                            tags.extend(metadata::keywords_from(&tool.read(Path::new(&c.path))?));
                        }
                    }
                    meta["XMP-dc:Subject"] = json!(tags);
                    let mut folders = vec![root
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string()];
                    if let Ok(relative) = path.parent().unwrap().strip_prefix(root) {
                        folders.extend(
                            relative
                                .components()
                                .map(|c| c.as_os_str().to_string_lossy().to_string()),
                        );
                    }
                    self.db.execute("INSERT INTO files(source,root,size,mtime,metadata,folders,companions,kind) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(source) DO UPDATE SET size=excluded.size,mtime=excluded.mtime,metadata=excluded.metadata,folders=excluded.folders,companions=excluded.companions,hash=NULL,error=NULL",params![source,metadata::path_string(root)?,size,mtime,meta.to_string(),serde_json::to_string(&folders)?,serde_json::to_string(&companions)?,if metadata::is_video(&detected){"video"}else{"photo"}])?;
                    self.db
                        .execute("DELETE FROM scan_errors WHERE path=?1", [source])?;
                    Ok(())
                })();
                if count.is_multiple_of(250) {
                    transaction.take().unwrap().commit()?;
                    transaction = Some(self.db.unchecked_transaction()?);
                }
                if let Err(e) = result {
                    self.db.execute(
                        "DELETE FROM files WHERE source=?1",
                        [path.to_string_lossy()],
                    )?;
                    self.scan_error(path, &format!("{e:#}"))?;
                }
            }
        }
        transaction.take().unwrap().commit()?;
        // Hash only candidates of equal size; never load the candidate list into RAM.
        self.db.execute_batch("DROP TABLE IF EXISTS temp.hash_candidates; CREATE TEMP TABLE hash_candidates AS SELECT id,source FROM files WHERE size IN (SELECT size FROM files GROUP BY size HAVING COUNT(*)>1); CREATE UNIQUE INDEX hash_candidate_id ON hash_candidates(id);")?;
        let mut last = 0i64;
        let mut transaction = Some(self.db.unchecked_transaction()?);
        loop {
            check_cancel(cancel)?;
            let row: Option<(i64, String)> = self
                .db
                .query_row(
                    "SELECT id,source FROM hash_candidates WHERE id>?1 ORDER BY id LIMIT 1",
                    [last],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let Some((id, path)) = row else {
                break;
            };
            last = id;
            progress(&mut notify, "duplicates", id as u64, count, &path);
            if id % 250 == 0 {
                transaction.take().unwrap().commit()?;
                transaction = Some(self.db.unchecked_transaction()?);
            }
            match hash_file(Path::new(&path), cancel) {
                Ok(hash) => {
                    self.db
                        .execute("UPDATE files SET hash=?1 WHERE id=?2", params![hash, id])?;
                }
                Err(e) => self.scan_error(Path::new(&path), &e.to_string())?,
            }
        }
        transaction.take().unwrap().commit()?;
        self.set("scan_state", "complete")?;
        progress(&mut notify, "scan complete", count, count, "");
        Ok(())
    }
    fn scan_error(&self, path: &Path, error: &str) -> Result<()> {
        self.db.execute("INSERT INTO scan_errors VALUES(?1,?2) ON CONFLICT(path) DO UPDATE SET error=excluded.error",params![path.to_string_lossy(),error])?;
        Ok(())
    }
    pub fn plan(&self, recipe: &Recipe, cancel: &AtomicBool, mut notify: Notify<'_>) -> Result<()> {
        let _lock = self.lock()?;
        recipe.validate()?;
        if self.setting("scan_state")?.as_deref() != Some("complete") {
            bail!("Finish scanning before planning");
        }
        let begun: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM items WHERE status IN ('done','publishing','cleanup','error')",
            [],
            |r| r.get(0),
        )?;
        if begun > 0 {
            bail!("An export has started; resume it or create a new session");
        }
        let output = PathBuf::from(self.setting("output")?.context("Missing output")?);
        self.db.execute_batch("BEGIN IMMEDIATE; DELETE FROM reservations; DELETE FROM collision_counters; DELETE FROM items; DELETE FROM artifacts; DELETE FROM cleanup; COMMIT; DROP TABLE IF EXISTS temp.resolution_candidates; CREATE TEMP TABLE resolution_candidates(id INTEGER PRIMARY KEY,key TEXT NOT NULL,rank INTEGER NOT NULL);")?;
        self.set("plan_state", "planning")?;
        self.set("recipe", &serde_json::to_string(recipe)?)?;
        let total: i64 = self
            .db
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
        let mut transaction = Some(self.db.unchecked_transaction()?);
        if output.exists() {
            for entry in WalkDir::new(&output)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| e.file_name() != ".photo-hat-staging")
            {
                check_cancel(cancel)?;
                let entry = entry?;
                if entry.file_type().is_dir() {
                    continue;
                }
                let key = entry
                    .path()
                    .with_extension("")
                    .to_string_lossy()
                    .nfc()
                    .collect::<String>()
                    .to_lowercase();
                self.db
                    .execute("INSERT OR IGNORE INTO reservations VALUES(?1,0)", [key])?;
            }
        }
        let mut last = 0i64;
        loop {
            check_cancel(cancel)?;
            let row: Option<PlanRow> = self.db.query_row("SELECT id,source,metadata,folders,mtime,hash,companions,root FROM files WHERE id>?1 ORDER BY id LIMIT 1",[last],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))).optional()?;
            let Some((id, source, meta, folders, mtime, hash, companion_json, root)) = row else {
                break;
            };
            last = id;
            let meta: Value = serde_json::from_str(&meta)?;
            let folders: Vec<String> = serde_json::from_str(&folders)?;
            let fallback = mtime
                .parse::<u128>()
                .ok()
                .and_then(|n| DateTime::<Utc>::from_timestamp((n / 1_000_000_000) as i64, 0))
                .map(|d| d.naive_utc());
            let (relative, warning) = recipe.destination(
                &meta,
                Path::new(&source),
                metadata::is_video(&extension(Path::new(&source))),
                fallback,
            )?;
            let tags = recipe.tags(&meta, &folders);
            if recipe.skip_lower_resolution_variants {
                if let Some((key, rank)) =
                    recipe::resolution_variant(Path::new(&source), Path::new(&root))
                {
                    self.db.execute(
                        "INSERT INTO resolution_candidates VALUES(?1,?2,?3)",
                        params![id, key, rank],
                    )?;
                }
            }
            let mut status = "planned";
            if let Some(hash) = &hash {
                let first: i64 =
                    self.db
                        .query_row("SELECT MIN(id) FROM files WHERE hash=?1", [hash], |r| {
                            r.get(0)
                        })?;
                if first != id {
                    let first_comp: String = self.db.query_row(
                        "SELECT companions FROM files WHERE id=?1",
                        [first],
                        |r| r.get(0),
                    )?;
                    if companion_signature(&first_comp)? != companion_signature(&companion_json)? {
                        self.db.execute("UPDATE items SET status='review',warning='Duplicate companions differ; choose a resolution' WHERE id=?1",[first])?;
                        status = "review";
                    } else {
                        let first_status: String = self.db.query_row(
                            "SELECT status FROM items WHERE id=?1",
                            [first],
                            |r| r.get(0),
                        )?;
                        status = if first_status == "review" {
                            "review"
                        } else {
                            "duplicate"
                        };
                    }
                    let old: String = self.db.query_row(
                        "SELECT keywords FROM items WHERE id=?1",
                        [first],
                        |r| r.get(0),
                    )?;
                    let mut merged: BTreeSet<String> = serde_json::from_str::<Vec<String>>(&old)?
                        .into_iter()
                        .collect();
                    merged.extend(tags.clone());
                    self.db.execute(
                        "UPDATE items SET keywords=?1 WHERE id=?2",
                        params![serde_json::to_string(&merged)?, first],
                    )?;
                }
            }
            let dest = self.reserve(id, &output, &relative)?;
            self.db.execute(
                "INSERT INTO items(id,destination,keywords,status,warning) VALUES(?1,?2,?3,?4,?5)",
                params![
                    id,
                    metadata::path_string(&dest)?,
                    serde_json::to_string(&tags)?,
                    status,
                    warning
                ],
            )?;
            if id % 250 == 0 {
                transaction.take().unwrap().commit()?;
                transaction = Some(self.db.unchecked_transaction()?);
            }
            if id % 50 == 1 {
                progress(&mut notify, "plan", id as u64, total as u64, &source);
            }
        }
        transaction.take().unwrap().commit()?;
        // Propagate conflicts to all members, including earlier duplicate rows.
        self.db.execute("UPDATE items SET status='review',warning='Duplicate companions differ; choose a resolution' WHERE id IN (SELECT f.id FROM files f WHERE f.hash IN (SELECT f2.hash FROM files f2 JOIN items i ON i.id=f2.id WHERE i.status='review'))",[])?;
        if recipe.skip_lower_resolution_variants {
            self.db.execute(
                "UPDATE items SET status='skipped',warning='Lower-resolution web/Instagram variant skipped; source retained' \
                 WHERE status IN ('planned','duplicate') AND id IN ( \
                   SELECT low.id FROM resolution_candidates low JOIN files low_file ON low_file.id=low.id \
                   WHERE low.rank=0 AND EXISTS ( \
                     SELECT 1 FROM resolution_candidates preferred JOIN files preferred_file ON preferred_file.id=preferred.id \
                     WHERE preferred.key=low.key AND preferred.rank>low.rank \
                       AND (low_file.hash IS NULL OR preferred_file.hash IS NULL OR low_file.hash<>preferred_file.hash) \
                   ) \
                 )",
                [],
            )?;
        }
        self.set("plan_state", "ready")?;
        progress(&mut notify, "plan ready", total as u64, total as u64, "");
        Ok(())
    }
    fn reserve(&self, id: i64, output: &Path, relative: &Path) -> Result<PathBuf> {
        let parent = output.join(relative.parent().context("Invalid recipe path")?);
        let stem = relative.file_stem().unwrap().to_string_lossy();
        let ext = relative.extension().unwrap().to_string_lossy();
        let counter = parent
            .join(stem.as_ref())
            .to_string_lossy()
            .nfc()
            .collect::<String>()
            .to_lowercase();
        let next: i64 = self
            .db
            .query_row(
                "SELECT next FROM collision_counters WHERE path=?1",
                [&counter],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        for n in next..(next + 1_000_000) {
            let stem = if n == 0 {
                stem.to_string()
            } else {
                format!("{stem}__{n}")
            };
            let candidate = parent.join(format!("{stem}.{ext}"));
            let key = parent.join(&stem).to_string_lossy().to_lowercase();
            let reserved: bool = self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM reservations WHERE path=?1)",
                [&key],
                |r| r.get(0),
            )?;
            let disk_conflict = false;
            if !reserved && !disk_conflict {
                self.db
                    .execute("INSERT INTO reservations VALUES(?1,?2)", params![key, id])?;
                self.db.execute("INSERT INTO collision_counters VALUES(?1,?2) ON CONFLICT(path) DO UPDATE SET next=excluded.next",params![counter,n+1])?;
                return Ok(candidate);
            }
        }
        bail!("Too many destination collisions")
    }
    pub fn resolve(&self, hash: &str, decision: &str) -> Result<()> {
        let _lock = self.lock()?;
        if !["keep_all", "use_first"].contains(&decision) {
            bail!("Choose keep_all or use_first");
        }
        let started:i64=self.db.query_row("SELECT COUNT(*) FROM items JOIN files USING(id) WHERE hash=?1 AND status NOT IN ('review','duplicate','planned')",[hash],|r|r.get(0))?;
        if started > 0 {
            bail!("Cannot change a group after export has started");
        }
        self.db.execute("UPDATE items SET status=CASE WHEN ?1='keep_all' OR id=(SELECT MIN(id) FROM files WHERE hash=?2) THEN 'planned' ELSE 'duplicate' END,warning='Duplicate decision reviewed' WHERE id IN (SELECT id FROM files WHERE hash=?2)",params![decision,hash])?;
        Ok(())
    }
    pub fn items(&self, after: i64, status: Option<&str>, limit: u32) -> Result<Vec<Item>> {
        let mut q=self.db.prepare("SELECT f.id,f.source,i.destination,i.status,i.keywords,i.warning,i.error,f.hash,f.size,f.kind,f.companions,f.metadata FROM items i JOIN files f USING(id) WHERE f.id>?1 AND (?2 IS NULL OR i.status=?2) ORDER BY f.id LIMIT ?3")?;
        let rows = q.query_map(params![after, status, limit.clamp(1, 500)], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, u64>(8)?,
                r.get::<_, String>(9)?,
                r.get::<_, String>(10)?,
                r.get::<_, String>(11)?,
            ))
        })?;
        let mut result = vec![];
        for row in rows {
            let (
                id,
                source,
                destination,
                status,
                keywords,
                warning,
                error,
                hash,
                size,
                kind,
                companions,
                metadata,
            ) = row?;
            result.push(Item {
                id,
                source,
                destination,
                status,
                keywords: serde_json::from_str(&keywords)?,
                warning,
                error,
                hash,
                size,
                kind,
                companions: serde_json::from_str(&companions)?,
                metadata: serde_json::from_str(&metadata)?,
            });
        }
        Ok(result)
    }
    pub fn report(&self, writer: &mut impl std::io::Write) -> Result<()> {
        writeln!(
            writer,
            "{}",
            json!({"type":"summary","data":self.summary()?})
        )?;
        let mut after = 0;
        loop {
            let rows = self.items(after, None, 500)?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                after = row.id;
                let hashes: (Option<String>, Option<String>) = self.db.query_row(
                    "SELECT before_hash,after_hash FROM items WHERE id=?1",
                    [row.id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                writeln!(
                    writer,
                    "{}",
                    json!({"type":"file","data":row,"before_hash":hashes.0,"after_hash":hashes.1})
                )?;
            }
        }
        let mut after = String::new();
        loop {
            let rows = self.scan_errors(&after)?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                after = row["path"].as_str().unwrap_or("").into();
                writeln!(writer, "{}", json!({"type":"scan_error","data":row}))?;
            }
        }
        Ok(())
    }
}
fn companion_signature(value: &str) -> Result<Vec<(String, String)>> {
    let list: Vec<Companion> = serde_json::from_str(value)?;
    Ok(list.into_iter().map(|c| (c.extension, c.hash)).collect())
}
pub(crate) fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|p| p.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}
pub(crate) fn fingerprint(path: &Path) -> Result<(u64, String)> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() {
        bail!("Source is no longer a regular file");
    }
    Ok((
        meta.len(),
        meta.modified()?
            .duration_since(UNIX_EPOCH)?
            .as_nanos()
            .to_string(),
    ))
}
pub fn hash_file(path: &Path, cancel: &AtomicBool) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        check_cancel(cancel)?;
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
fn companions(path: &Path) -> Result<Vec<Companion>> {
    let mut paths = BTreeSet::new();
    let mut inodes = BTreeSet::new();
    use std::os::unix::fs::MetadataExt;
    for ext in ["xmp", "XMP", "acr", "ACR"] {
        for p in [
            path.with_extension(ext),
            PathBuf::from(format!("{}.{}", path.display(), ext)),
        ] {
            if p.is_file() {
                let m = fs::metadata(&p)?;
                if inodes.insert((m.dev(), m.ino())) {
                    paths.insert(p);
                }
            }
        }
    }
    let mut result = vec![];
    for path in paths {
        fingerprint(&path)?;
        result.push(Companion {
            path: metadata::path_string(&path)?,
            extension: extension(&path),
            hash: hash_file(&path, &AtomicBool::new(false))?,
        });
    }
    // Multiple packets with the same role cannot be merged without risking edits.
    let roles: BTreeSet<_> = result.iter().map(|c| &c.extension).collect();
    if roles.len() != result.len() {
        bail!(
            "Multiple sidecars with the same extension; resolve their precedence before scanning"
        );
    }
    result.sort_by(|a, b| a.extension.cmp(&b.extension));
    Ok(result)
}
pub fn absolute_directory(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return fs::canonicalize(path).context("Cannot resolve directory");
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut ancestor = absolute.as_path();
    let mut suffix = vec![];
    while !ancestor.exists() {
        suffix.push(
            ancestor
                .file_name()
                .context("Invalid directory")?
                .to_owned(),
        );
        ancestor = ancestor.parent().context("Invalid directory")?;
    }
    let mut resolved = fs::canonicalize(ancestor)?;
    for part in suffix.iter().rev() {
        if part == ".." || part == "." {
            bail!("Directory traversal is not supported");
        }
        resolved.push(part);
    }
    Ok(resolved)
}
