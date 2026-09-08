use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex},
};

/// One long-lived, bounded ExifTool worker. Arguments never pass through a shell.
pub struct MetadataTool {
    child: Child,
    input: ChildStdin,
    output: std::sync::mpsc::Receiver<String>,
    stderr: Arc<Mutex<String>>,
    sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormatSupport {
    pub extension: String,
    pub read: bool,
    pub embed: bool,
    pub preview: String,
}

pub fn tool_command() -> Command {
    if let Some(root) = std::env::var_os("PHOTO_HAT_METADATA_DIR") {
        let root = PathBuf::from(root);
        let mut command = Command::new(root.join("perl"));
        command.arg(root.join("exiftool"));
        let libs = [
            root.join("lib"),
            root.join("perl-lib"),
            root.join("perl-arch"),
        ];
        command.env("PERL5LIB", std::env::join_paths(libs).unwrap());
        command.env("LD_LIBRARY_PATH", root.join("native"));
        command
    } else {
        Command::new(std::env::var_os("PHOTO_HAT_EXIFTOOL").unwrap_or_else(|| "exiftool".into()))
    }
}

impl MetadataTool {
    pub fn new() -> Result<Self> {
        let mut child = tool_command()
            .args(["-config", "", "-stay_open", "True", "-@", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Cannot start ExifTool. Install it or configure PHOTO_HAT_METADATA_DIR.")?;
        let stdout = child.stdout.take().unwrap();
        let stderr = Arc::new(Mutex::new(String::new()));
        let stderr_copy = Arc::clone(&stderr);
        let child_stderr = child.stderr.take().unwrap();
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = std::io::Read::read_to_string(&mut BufReader::new(child_stderr), &mut text);
            if let Ok(mut saved) = stderr_copy.lock() {
                *saved = text;
            }
        });
        let (sender, output) = std::sync::mpsc::sync_channel(128);
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) => {
                        if sender.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            input: child.stdin.take().unwrap(),
            output,
            stderr,
            child,
            sequence: 0,
        })
    }
    pub fn execute(&mut self, args: &[String]) -> Result<String> {
        for arg in args {
            if arg.contains(['\n', '\r']) {
                bail!("Newlines in metadata arguments are unsupported");
            }
        }
        self.sequence += 1;
        for arg in args {
            writeln!(self.input, "{arg}")
                .map_err(|error| self.process_error("writing command arguments", error))?;
        }
        writeln!(self.input, "-execute{}", self.sequence)
            .map_err(|error| self.process_error("writing execute marker", error))?;
        self.input
            .flush()
            .map_err(|error| self.process_error("flushing command", error))?;
        let ready = format!("{{ready{}}}", self.sequence);
        let mut result = String::new();
        loop {
            // Large PSD/TIFF files can require several minutes for ExifTool to
            // rewrite. Keep the worker alive long enough for that operation,
            // while still recovering from a genuinely wedged process.
            let timeout_seconds = std::env::var("PHOTO_HAT_EXIFTOOL_TIMEOUT_SECS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|value| *value >= 10)
                .unwrap_or(600);
            let line = match self
                .output
                .recv_timeout(std::time::Duration::from_secs(timeout_seconds))
            {
                Ok(line) => line,
                Err(e) => {
                    let detail = self
                        .stderr
                        .lock()
                        .ok()
                        .map(|s| s.trim().to_owned())
                        .unwrap_or_default();
                    let _ = self.child.kill();
                    bail!(
                        "ExifTool timed out or exited: {e}{}",
                        if detail.is_empty() {
                            String::new()
                        } else {
                            format!(": {detail}")
                        }
                    );
                }
            };
            if line.trim_end() == ready {
                break;
            }
            result.push_str(&line);
            result.push('\n');
        }
        Ok(result)
    }
    fn process_error(&mut self, action: &str, error: std::io::Error) -> anyhow::Error {
        let detail = self
            .stderr
            .lock()
            .ok()
            .map(|s| s.trim().to_owned())
            .unwrap_or_default();
        let status = self.child.try_wait().ok().flatten().map(|s| s.to_string());
        anyhow!(
            "ExifTool failed while {action}: {error}{}{}",
            status.map(|s| format!(" (status {s})")).unwrap_or_default(),
            if detail.is_empty() {
                String::new()
            } else {
                format!(": {detail}")
            }
        )
    }
    pub fn read(&mut self, path: &Path) -> Result<Value> {
        let output = self.execute(&[
            "-json".into(),
            "-G1".into(),
            "-n".into(),
            "-charset".into(),
            "filename=UTF8".into(),
            path_string(path)?,
        ])?;
        let items: Vec<Value> =
            serde_json::from_str(&output).context("Invalid ExifTool response")?;
        let value = items
            .into_iter()
            .next()
            .context("Empty metadata response")?;
        if let Some(error) = get(&value, &["Error"]) {
            bail!("ExifTool: {error}");
        }
        Ok(value)
    }
    pub fn extensions(&mut self) -> Result<BTreeSet<String>> {
        Ok(self
            .execute(&["-listf".into()])?
            .split_whitespace()
            .filter(|x| x.chars().all(|c| c.is_ascii_alphanumeric()))
            .map(|x| x.to_ascii_lowercase())
            .collect())
    }
    pub fn write_keywords(&mut self, path: &Path, keywords: &[String], iptc: bool) -> Result<()> {
        let mut args = vec![
            "-overwrite_original".into(),
            "-charset".into(),
            "filename=UTF8".into(),
            "-XMP-dc:Subject=".into(),
        ];
        for keyword in keywords {
            args.push(format!("-XMP-dc:Subject+={keyword}"));
        }
        if iptc {
            args.push("-IPTC:CodedCharacterSet=UTF8".into());
            args.push("-IPTC:Keywords=".into());
            for keyword in keywords.iter().filter(|x| x.len() <= 64) {
                args.push(format!("-IPTC:Keywords+={keyword}"));
            }
        }
        args.push(path_string(path)?);
        self.execute(&args)?;
        let actual = keywords_from(&self.read(path)?);
        for keyword in keywords {
            if !actual.contains(keyword) {
                bail!("Keyword read-back failed for {keyword:?}");
            }
        }
        Ok(())
    }
    pub fn create_xmp(&mut self, source: &Path, destination: &Path) -> Result<()> {
        self.execute(&[
            "-tagsFromFile".into(),
            path_string(source)?,
            "-all:all".into(),
            "-o".into(),
            path_string(destination)?,
        ])?;
        if !destination.is_file() {
            // A valid empty packet is needed for files with no transferable metadata.
            std::fs::write(destination, "<?xpacket begin='\u{feff}' id='W5M0MpCehiHzreSzNTczkc9d'?><x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description rdf:about=''/></rdf:RDF></x:xmpmeta><?xpacket end='w'?>")?;
        }
        self.read(destination)?;
        Ok(())
    }
}
impl Drop for MetadataTool {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn path_string(path: &Path) -> Result<String> {
    let value = path
        .to_str()
        .context("Non-UTF-8 paths are not supported; rename this file before retrying")?;
    if value.contains(['\r', '\n']) {
        bail!("Paths containing newlines are not supported");
    }
    Ok(value.to_owned())
}
pub fn get(value: &Value, names: &[&str]) -> Option<String> {
    for name in names {
        if let Some(map) = value.as_object() {
            for (key, value) in map {
                if key == name || (!name.contains(':') && key.rsplit(':').next() == Some(name)) {
                    if let Some(s) = value.as_str() {
                        return Some(s.to_owned());
                    }
                    if value.is_number() {
                        return Some(value.to_string());
                    }
                }
            }
        }
    }
    None
}
pub fn keywords_from(value: &Value) -> Vec<String> {
    let mut result = BTreeSet::new();
    if let Some(map) = value.as_object() {
        for (key, value) in map {
            if ["Subject", "Keywords"].contains(&key.rsplit(':').next().unwrap_or("")) {
                if let Some(array) = value.as_array() {
                    for value in array {
                        if let Some(s) = value.as_str() {
                            result.insert(s.to_owned());
                        }
                    }
                } else if let Some(s) = value.as_str() {
                    result.insert(s.to_owned());
                }
            }
        }
    }
    result.into_iter().collect()
}
pub fn is_video(extension: &str) -> bool {
    [
        "mov", "mp4", "m4v", "avi", "mkv", "mts", "m2ts", "mxf", "webm", "3gp", "mpg", "mpeg",
    ]
    .contains(&extension)
}
pub fn is_photo(extension: &str) -> bool {
    [
        "jpg", "jpeg", "tif", "tiff", "dng", "png", "heic", "heif", "webp", "avif", "psd", "cr2",
        "cr3", "crw", "nef", "nrw", "arw", "sr2", "srf", "raf", "orf", "rw2", "raw", "pef", "ptx",
        "rwl", "3fr", "fff", "iiq", "mos", "mrw", "srw", "x3f", "erf", "kdc", "dcr",
    ]
    .contains(&extension)
}
pub fn can_embed(extension: &str) -> bool {
    ["jpg", "jpeg", "dng", "png", "heic", "heif", "webp"].contains(&extension)
}
pub fn capabilities() -> Result<Vec<FormatSupport>> {
    let extensions = MetadataTool::new()?.extensions()?;
    Ok(extensions
        .into_iter()
        .filter(|x| is_photo(x) || is_video(x))
        .map(|extension| FormatSupport {
            embed: can_embed(&extension),
            preview: "metadata-only".into(),
            read: true,
            extension,
        })
        .collect())
}
