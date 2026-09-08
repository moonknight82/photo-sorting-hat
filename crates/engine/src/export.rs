use super::*;
use std::io::{Read, Write};

impl Engine {
    pub fn export(&self, cancel: &AtomicBool, mut notify: Notify<'_>) -> Result<()> {
        let _lock = self.lock()?;
        if self.setting("plan_state")?.as_deref() != Some("ready") {
            bail!("Create a complete export plan first");
        }
        let reviews: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM items WHERE status='review'",
            [],
            |r| r.get(0),
        )?;
        if reviews > 0 {
            bail!("Resolve {reviews} files with conflicting duplicate sidecars before export");
        }
        let recipe: Recipe =
            serde_json::from_str(&self.setting("recipe")?.context("Missing recipe")?)?;
        let output = PathBuf::from(self.setting("output")?.context("Missing output")?);
        fs::create_dir_all(&output)?;
        if fs::canonicalize(&output)? != output {
            bail!("Output folder changed since planning");
        }
        let session = match self.setting("session")? {
            Some(id) => id,
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                self.set("session", &id)?;
                id
            }
        };
        let staging = output.join(".photo-hat-staging").join(session);
        safe_directory(&output, &staging)?;
        let mut tool = metadata::MetadataTool::new()?;
        let total: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM items WHERE status NOT IN ('duplicate','skipped')",
            [],
            |r| r.get(0),
        )?;
        let mut after = 0;
        let mut failures = 0;
        loop {
            check_cancel(cancel)?;
            let mut rows = self.items(after, None, 1)?;
            let Some(item) = rows.pop() else {
                break;
            };
            after = item.id;
            if item.status == "done" {
                continue;
            }
            if item.status == "skipped" {
                continue;
            }
            if metadata::get(&item.metadata, &["FileType"]).as_deref() == Some("MacOS")
                || Path::new(&item.source)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("._"))
            {
                self.db.execute(
                    "UPDATE items SET status='skipped',warning='Skipped macOS AppleDouble metadata file',error=NULL WHERE id=?1",
                    [item.id],
                )?;
                continue;
            }
            if item.status == "duplicate" {
                let expected: (u64, String) = self.db.query_row(
                    "SELECT size,mtime FROM files WHERE id=?1",
                    [item.id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                let checked = (|| -> Result<()> {
                    if fingerprint(Path::new(&item.source))? != expected
                        || Some(hash_file(Path::new(&item.source), cancel)?) != item.hash
                    {
                        bail!("Duplicate source changed since scan; retained and requires a new session");
                    }
                    Ok(())
                })();
                if let Err(e) = checked {
                    check_cancel(cancel)?;
                    self.db.execute(
                        "UPDATE items SET status='error',error=?1 WHERE id=?2",
                        params![e.to_string(), item.id],
                    )?;
                    failures += 1;
                }
                continue;
            }
            progress(
                &mut notify,
                "export",
                item.id as u64,
                total as u64,
                &item.source,
            );
            let result = (|| -> Result<()> {
                let artifact_count: i64 = self.db.query_row(
                    "SELECT COUNT(*) FROM artifacts WHERE item=?1",
                    [item.id],
                    |r| r.get(0),
                )?;
                if artifact_count == 0 {
                    self.stage(&item, &recipe, &staging, &mut tool, cancel)?;
                }
                progress(
                    &mut notify,
                    "publish",
                    item.id as u64,
                    total as u64,
                    &item.source,
                );
                check_cancel(cancel)?;
                self.publish(&item, &output, cancel)?;
                if recipe.operation == "move" {
                    self.cleanup_sources(&item, cancel)?;
                }
                self.db.execute(
                    "UPDATE items SET status='done',error=NULL WHERE id=?1",
                    [item.id],
                )?;
                Ok(())
            })();
            if let Err(e) = result {
                if cancel.load(Ordering::Relaxed) {
                    return Err(e);
                }
                self.db.execute(
                    "UPDATE items SET status='error',error=?1 WHERE id=?2",
                    params![format!("{e:#}"), item.id],
                )?;
                failures += 1;
            }
        }
        progress(
            &mut notify,
            if failures == 0 {
                "export complete"
            } else {
                "export needs attention"
            },
            total as u64,
            total as u64,
            "",
        );
        if failures > 0 {
            bail!(
                "{failures} files need attention. Review errors and resume after resolving them."
            );
        }
        Ok(())
    }
    fn stage(
        &self,
        item: &Item,
        recipe: &Recipe,
        staging: &Path,
        tool: &mut metadata::MetadataTool,
        cancel: &AtomicBool,
    ) -> Result<()> {
        let source = Path::new(&item.source);
        let expected: (u64, String) =
            self.db
                .query_row("SELECT size,mtime FROM files WHERE id=?1", [item.id], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?;
        if fingerprint(source)? != expected {
            bail!("Source changed since scan; use a new session");
        }
        let before = hash_file(source, cancel)?;
        if item.hash.as_ref().is_some_and(|h| h != &before) {
            bail!("Source content changed since duplicate analysis");
        }
        for companion in &item.companions {
            if hash_file(Path::new(&companion.path), cancel)? != companion.hash {
                bail!("Companion changed since scan: {}", companion.path);
            }
        }
        let dir = staging.join(format!("{}-{}", item.id, uuid::Uuid::new_v4()));
        fs::create_dir(&dir)?;
        let dest = Path::new(&item.destination);
        let media = dir.join(dest.file_name().context("Invalid destination")?);
        let detected = metadata::get(&item.metadata, &["FileTypeExtension"])
            .unwrap_or_else(|| extension(source))
            .to_lowercase();
        let named = extension(source);
        let same_format = named == detected
            || matches!(
                (named.as_str(), detected.as_str()),
                ("jpeg", "jpg")
                    | ("jpg", "jpeg")
                    | ("tiff", "tif")
                    | ("tif", "tiff")
                    | ("heif", "heic")
                    | ("heic", "heif")
            );
        // ExifTool selects some write behavior from the filename suffix. Keep
        // mislabeled media byte-identical and rely on its verified XMP sidecar.
        let embed = same_format && metadata::can_embed(&detected);
        let linked = recipe.operation == "move" && !embed && fs::hard_link(source, &media).is_ok();
        if !linked {
            copy_checked(source, &media, cancel)?;
        }
        if hash_file(&media, cancel)? != before || fingerprint(source)? != expected {
            bail!("Source changed while copying or verification failed");
        }
        let mut artifacts = vec![];
        for companion in &item.companions {
            let stage = media.with_extension(&companion.extension);
            copy_checked(Path::new(&companion.path), &stage, cancel)?;
            if hash_file(&stage, cancel)? != companion.hash {
                bail!("Sidecar verification failed");
            }
            artifacts.push((stage, dest.with_extension(&companion.extension)));
        }
        if item.kind == "photo" {
            let xmp = media.with_extension("xmp");
            if !xmp.exists() {
                tool.create_xmp(&media, &xmp)?;
                artifacts.push((xmp.clone(), dest.with_extension("xmp")));
            }
            tool.write_keywords(&xmp, &item.keywords, false)?;
            if embed {
                tool.write_keywords(
                    &media,
                    &item.keywords,
                    ["jpg", "jpeg", "tif", "tiff"].contains(&detected.as_str()),
                )?;
            }
        }
        let after = hash_file(&media, cancel)?;
        artifacts.push((media, dest.to_path_buf()));
        let mut verified = vec![];
        for (stage, path) in artifacts {
            File::open(&stage)?.sync_all()?;
            let hash = hash_file(&stage, cancel)?;
            verified.push((stage, path, hash));
        }
        sync_dir(&dir)?;
        // Persist every expected output before exposing any output file.
        self.db.execute_batch("BEGIN IMMEDIATE")?;
        let record = (|| -> Result<()> {
            for (stage, path, hash) in &verified {
                self.db.execute(
                    "INSERT INTO artifacts VALUES(?1,?2,?3,?4,?5)",
                    params![
                        item.id,
                        metadata::path_string(path)?,
                        metadata::path_string(stage)?,
                        hash,
                        file_identity(stage)?
                    ],
                )?;
            }
            if recipe.operation == "move" {
                self.db.execute(
                    "INSERT INTO cleanup(item,path,hash) VALUES(?1,?2,?3)",
                    params![item.id, item.source, before],
                )?;
                for c in &item.companions {
                    // Shared RAW/JPEG sidecars remain until every referring file is exported.
                    self.db.execute(
                        "INSERT INTO cleanup(item,path,hash) VALUES(?1,?2,?3)",
                        params![item.id, c.path, c.hash],
                    )?;
                }
            }
            self.db.execute("UPDATE items SET status='publishing',before_hash=?1,after_hash=?2,error=NULL WHERE id=?3",params![before,after,item.id])?;
            Ok(())
        })();
        if let Err(e) = record {
            self.db.execute_batch("ROLLBACK")?;
            return Err(e);
        }
        self.db.execute_batch("COMMIT")?;
        Ok(())
    }
    fn publish(&self, item: &Item, output: &Path, cancel: &AtomicBool) -> Result<()> {
        let mut query = self.db.prepare(
            "SELECT path,stage,hash,identity FROM artifacts WHERE item=?1 ORDER BY path",
        )?;
        let artifacts = query
            .query_map([item.id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        // Validate the entire bundle before publishing any of it.
        for (path, stage, hash, identity) in &artifacts {
            check_cancel(cancel)?;
            let path = Path::new(path);
            let stage = Path::new(stage);
            if !path.starts_with(output) {
                bail!("Destination escaped output root");
            }
            safe_directory(output, path.parent().context("Invalid parent")?)?;
            if path.exists() {
                // A resumed output must be the same staged inode recorded before publication.
                if file_identity(path)? != *identity {
                    bail!("Destination already exists: {}", path.display());
                }
            }
            let verify_path = if path.exists() { path } else { stage };
            if hash_file(verify_path, cancel)? != *hash {
                bail!("Staged data changed; source retained");
            }
        }
        for (path, stage, _, _) in &artifacts {
            let path = Path::new(path);
            let stage = Path::new(stage);
            if !path.exists() {
                publish_no_replace(stage, path).with_context(|| {
                    format!("Cannot publish without overwriting {}", path.display())
                })?;
            }
            sync_dir(path.parent().unwrap())?;
        }
        // Atomic no-replace renames work on external filesystems without hardlink support.
        self.db.execute(
            "UPDATE items SET status='cleanup',error=NULL WHERE id=?1",
            [item.id],
        )?;
        Ok(())
    }
    fn cleanup_sources(&self, item: &Item, cancel: &AtomicBool) -> Result<()> {
        let mut q = self
            .db
            .prepare("SELECT path,hash,done FROM cleanup WHERE item=?1")?;
        let rows = q
            .query_map([item.id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, bool>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        // Verify every destination again immediately before removing any source.
        let mut q = self
            .db
            .prepare("SELECT path,hash FROM artifacts WHERE item=?1")?;
        for row in q.query_map([item.id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })? {
            let (path, hash) = row?;
            if hash_file(Path::new(&path), cancel)? != hash {
                bail!("Output verification failed; source retained");
            }
        }
        for (path, hash, done) in rows {
            if done {
                continue;
            }
            check_cancel(cancel)?;
            // If another indexed media file refers to this companion, retain it conservatively.
            if path != item.source {
                let shared:bool=self.db.query_row("SELECT EXISTS(SELECT 1 FROM files,json_each(files.companions) j WHERE files.id!=?1 AND json_extract(j.value,'$.path')=?2)",params![item.id,path],|r|r.get(0))?;
                if shared {
                    self.db.execute(
                        "UPDATE cleanup SET done=1 WHERE item=?1 AND path=?2",
                        params![item.id, path],
                    )?;
                    continue;
                }
            }
            let source = Path::new(&path);
            match fs::symlink_metadata(source) {
                Ok(meta) => {
                    if !meta.is_file() || hash_file(source, cancel)? != hash {
                        bail!("Source changed before move cleanup; retained: {path}");
                    }
                    fs::remove_file(source)?;
                    sync_dir(source.parent().context("Invalid source parent")?)?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    if !source.parent().is_some_and(|p| p.is_dir()) {
                        bail!(
                            "Source drive is unavailable; reconnect before resuming move cleanup"
                        );
                    }
                }
                Err(e) => return Err(e.into()),
            }
            self.db.execute(
                "UPDATE cleanup SET done=1 WHERE item=?1 AND path=?2",
                params![item.id, path],
            )?;
        }
        Ok(())
    }
}
fn copy_checked(source: &Path, destination: &Path, cancel: &AtomicBool) -> Result<()> {
    let mut input = File::open(source)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        check_cancel(cancel)?;
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        output.write_all(&buffer[..n])?;
    }
    output.sync_all()?;
    Ok(())
}
fn safe_directory(root: &Path, directory: &Path) -> Result<()> {
    let relative = directory
        .strip_prefix(root)
        .context("Destination escaped output root")?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            bail!("Unsafe destination path");
        }
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(m) => {
                if !m.is_dir() || m.file_type().is_symlink() {
                    bail!(
                        "Destination contains a symlink or non-directory: {}",
                        current.display()
                    );
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&current)?,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn sync_dir(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}
fn file_identity(path: &Path) -> Result<String> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() {
        bail!("Output is not a regular file");
    }
    Ok(format!("{}:{}", metadata.dev(), metadata.ino()))
}
fn publish_no_replace(source: &Path, destination: &Path) -> Result<()> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        source,
        rustix::fs::CWD,
        destination,
        rustix::fs::RenameFlags::NOREPLACE,
    )?;
    Ok(())
}
