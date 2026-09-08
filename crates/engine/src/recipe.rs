use crate::metadata::{get, keywords_from};
use anyhow::{bail, Result};
use chrono::{Datelike, NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Recipe {
    pub version: u32,
    pub operation: String,
    pub folder_template: String,
    pub filename_template: String,
    pub photo_folder: String,
    pub video_folder: String,
    pub separate_video: bool,
    pub project: String,
    pub folder_tags: bool,
    /// Empty means all ancestors below each selected source, including its name.
    pub selected_folders: Vec<String>,
    pub excluded_folders: Vec<String>,
    pub keywords: Vec<String>,
    pub camera_tags: bool,
    pub filesystem_date_fallback: bool,
}
impl Default for Recipe {
    fn default() -> Self {
        Self {
            version: 1,
            operation: "copy".into(),
            folder_template: "{year}/{date}".into(),
            filename_template: "{stem}".into(),
            photo_folder: "Photos".into(),
            video_folder: "Video".into(),
            separate_video: true,
            project: String::new(),
            folder_tags: true,
            selected_folders: vec![],
            excluded_folders: vec![
                "DCIM".into(),
                "exports".into(),
                "Photos".into(),
                "Pictures".into(),
            ],
            keywords: vec![],
            camera_tags: false,
            filesystem_date_fallback: false,
        }
    }
}
pub fn clean(value: &str) -> String {
    let s: String = value
        .nfc()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let s = s.trim().trim_matches('.');
    let s: String = s.chars().take(120).collect();
    if s.is_empty() {
        "Unknown".into()
    } else {
        s
    }
}
fn render(template: &str, tokens: &[(&str, String)]) -> Result<String> {
    let mut rendered = template.to_owned();
    for (key, value) in tokens {
        rendered = rendered.replace(&format!("{{{key}}}"), value);
    }
    if rendered.contains(['{', '}']) {
        bail!("Unknown or malformed recipe token: {rendered}");
    }
    Ok(rendered)
}
impl Recipe {
    pub fn validate(&self) -> Result<()> {
        if !["copy", "move"].contains(&self.operation.as_str()) {
            bail!("Operation must be copy or move");
        }
        if self.version != 1 {
            bail!("Unsupported recipe version {}", self.version);
        }
        for value in [&self.photo_folder, &self.video_folder] {
            if value.is_empty() || clean(value) != *value {
                bail!("Photo/video folder names must be single safe directory names");
            }
        }
        if self.folder_template.starts_with('/')
            || self.folder_template.contains('\\')
            || self
                .folder_template
                .split('/')
                .any(|x| x == ".." || x == "." || x.is_empty())
        {
            bail!("Folder template must be a safe relative path");
        }
        if self.filename_template.contains(['/', '\\']) || self.filename_template.is_empty() {
            bail!("Filename template cannot contain directories");
        }
        if self
            .keywords
            .iter()
            .any(|x| x.contains(['\r', '\n']) || x.trim().is_empty())
        {
            bail!("Keywords must be nonempty single lines");
        }
        let tokens = tokens(&Value::Null, Path::new("example.jpg"), None, &self.project);
        render(&self.folder_template, &tokens)?;
        render(&self.filename_template, &tokens)?;
        Ok(())
    }
    pub fn destination(
        &self,
        metadata: &Value,
        source: &Path,
        video: bool,
        fallback: Option<NaiveDateTime>,
    ) -> Result<(PathBuf, Option<String>)> {
        self.validate()?;
        let date = capture_date(metadata).or(if self.filesystem_date_fallback {
            fallback
        } else {
            None
        });
        let tokens = tokens(metadata, source, date, &self.project);
        let mut path = PathBuf::from(if video && self.separate_video {
            &self.video_folder
        } else {
            &self.photo_folder
        });
        if date.is_some() {
            for part in render(&self.folder_template, &tokens)?.split('/') {
                path.push(clean(part));
            }
        } else {
            path.push("Undated");
        }
        let stem = clean(&render(&self.filename_template, &tokens)?);
        path.push(format!(
            "{stem}.{}",
            source.extension().and_then(|x| x.to_str()).unwrap_or("bin")
        ));
        let warning = if date.is_none() {
            Some("No reliable capture date; review Undated".into())
        } else if capture_date(metadata).is_none() {
            Some("Using filesystem modification date".into())
        } else if get(metadata, &["OffsetTimeOriginal", "OffsetTime", "TimeZone"]).is_none() {
            Some("Timezone unspecified; recorded clock time preserved".into())
        } else {
            None
        };
        Ok((path, warning))
    }
    pub fn tags(&self, metadata: &Value, folders: &[String]) -> Vec<String> {
        let mut tags: BTreeSet<String> = keywords_from(metadata).into_iter().collect();
        tags.extend(self.keywords.iter().map(|s| s.trim().to_owned()));
        if self.folder_tags {
            tags.extend(
                folders
                    .iter()
                    .filter(|name| {
                        !self
                            .excluded_folders
                            .iter()
                            .any(|x| x.eq_ignore_ascii_case(name))
                            && (self.selected_folders.is_empty()
                                || self.selected_folders.contains(name))
                    })
                    .cloned(),
            );
        }
        if self.camera_tags {
            for tag in ["Make", "Model", "LensModel"] {
                if let Some(value) = get(metadata, &[tag]) {
                    tags.insert(value);
                }
            }
        }
        tags.into_iter().filter(|s| !s.is_empty()).collect()
    }
}
pub fn capture_date(metadata: &Value) -> Option<NaiveDateTime> {
    for name in [
        "ExifIFD:DateTimeOriginal",
        "DateTimeOriginal",
        "SubSecDateTimeOriginal",
        "CreationDate",
        "CreateDate",
        "MediaCreateDate",
        "TrackCreateDate",
    ] {
        if let Some(value) = get(metadata, &[name]) {
            if let Some(prefix) = value.get(..19) {
                for format in [
                    "%Y:%m:%d %H:%M:%S",
                    "%Y-%m-%dT%H:%M:%S",
                    "%Y-%m-%d %H:%M:%S",
                ] {
                    if let Ok(date) = NaiveDateTime::parse_from_str(prefix, format) {
                        if date.year() >= 1800 {
                            return Some(date);
                        }
                    }
                }
            }
            if value.len() == 10 {
                if let Ok(date) = NaiveDate::parse_from_str(&value, "%Y:%m:%d") {
                    return date.and_hms_opt(0, 0, 0);
                }
            }
        }
    }
    None
}
fn tokens(
    metadata: &Value,
    path: &Path,
    date: Option<NaiveDateTime>,
    project: &str,
) -> Vec<(&'static str, String)> {
    let date_token = |format: &str| {
        date.map(|d| d.format(format).to_string())
            .unwrap_or_else(|| "Undated".into())
    };
    vec![
        ("year", date_token("%Y")),
        ("month", date_token("%m")),
        ("day", date_token("%d")),
        ("date", date_token("%Y-%m-%d")),
        ("timestamp", date_token("%Y%m%d_%H%M%S")),
        (
            "camera",
            clean(&get(metadata, &["Model"]).unwrap_or_default()),
        ),
        ("make", clean(&get(metadata, &["Make"]).unwrap_or_default())),
        ("project", clean(project)),
        (
            "source_folder",
            clean(
                path.parent()
                    .and_then(|p| p.file_name())
                    .and_then(|p| p.to_str())
                    .unwrap_or("Unknown"),
            ),
        ),
        (
            "stem",
            clean(path.file_stem().and_then(|s| s.to_str()).unwrap_or("image")),
        ),
    ]
}
