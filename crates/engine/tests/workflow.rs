use photo_hat_engine::{
    hash_file,
    metadata::{self, MetadataTool},
    recipe::capture_date,
    Engine, Recipe,
};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use tempfile::TempDir;

struct Archive {
    _temp: TempDir,
    source: PathBuf,
    output: PathBuf,
    db: PathBuf,
}
impl Archive {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("Summer São Paulo");
        fs::create_dir(&source).unwrap();
        Self {
            source,
            output: temp.path().join("out"),
            db: temp.path().join("index.sqlite"),
            _temp: temp,
        }
    }
    fn add(&self, name: &str, fixture: &str) -> PathBuf {
        let path = self.source.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(fixture),
            &path,
        )
        .unwrap();
        path
    }
    fn engine(&self) -> Engine {
        Engine::open(&self.db).unwrap()
    }
    fn plan(&self, recipe: &Recipe) -> Engine {
        let e = self.engine();
        e.scan(
            std::slice::from_ref(&self.source),
            &self.output,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(
            e.summary().unwrap()["scan_errors"],
            0,
            "{}",
            e.summary().unwrap()
        );
        e.plan(recipe, &AtomicBool::new(false), &mut |_| {})
            .unwrap();
        e
    }
}
fn hash(p: &Path) -> String {
    hash_file(p, &AtomicBool::new(false)).unwrap()
}
fn date(p: &Path) {
    MetadataTool::new()
        .unwrap()
        .execute(&[
            "-overwrite_original".into(),
            "-DateTimeOriginal=2024:07:18 14:30:22".into(),
            p.to_str().unwrap().into(),
        ])
        .unwrap();
}
#[test]
fn dates_and_recipes_preserve_camera_clock() {
    let meta = json!({"ExifIFD:DateTimeOriginal":"2024:07:18 23:30:22","ExifIFD:OffsetTimeOriginal":"-03:00","IFD0:Model":"Canon / R5"});
    let recipe = Recipe::default();
    let (path, warning) = recipe
        .destination(&meta, Path::new("DSC_2048.CR3"), false, None)
        .unwrap();
    assert_eq!(path, Path::new("Photos/2024/2024-07-18/DSC_2048.CR3"));
    assert!(warning.is_none());
    assert!(capture_date(&json!({"DateTimeOriginal":"0000:00:00 00:00:00"})).is_none());
    assert!(Recipe {
        folder_template: "../../escape".into(),
        ..recipe.clone()
    }
    .validate()
    .is_err());
    assert!(Recipe {
        folder_template: "{unknown}".into(),
        ..recipe.clone()
    }
    .validate()
    .is_err());
    assert!(Recipe {
        operation: "delete".into(),
        ..recipe
    }
    .validate()
    .is_err());
}
#[test]
fn jpeg_round_trip_preserves_source_and_adds_sidecar_and_embedded_tags() {
    let a = Archive::new();
    let original = a.add("Photo.jpg", "sample.jpg");
    date(&original);
    let before = hash(&original);
    let recipe = Recipe {
        keywords: vec!["Portfolio".into(), "ação & <luz>".into()],
        ..Recipe::default()
    };
    let engine = a.plan(&recipe);
    engine
        .export(&AtomicBool::new(false), &mut |_| {})
        .unwrap_or_else(|error| panic!("{error:#}: {:?}", engine.items(0, None, 10).unwrap()));
    let item = engine.items(0, None, 10).unwrap().remove(0);
    assert_eq!(hash(&original), before);
    assert_eq!(item.status, "done");
    let destination = Path::new(&item.destination);
    assert!(destination.with_extension("xmp").is_file());
    let mut tool = MetadataTool::new().unwrap();
    for p in [destination.to_path_buf(), destination.with_extension("xmp")] {
        let tags = metadata::keywords_from(&tool.read(&p).unwrap());
        assert!(tags.contains(&"Portfolio".into()));
        assert!(tags.contains(&"Summer São Paulo".into()));
        assert!(tags.contains(&"ação & <luz>".into()));
    }
    engine
        .export(&AtomicBool::new(false), &mut |_| {})
        .unwrap_or_else(|error| panic!("{error:#}: {:?}", engine.items(0, None, 10).unwrap()));
    assert_eq!(hash(&original), before);
}
#[test]
fn numeric_folder_keywords_round_trip_as_exact_text() {
    let a = Archive::new();
    a.add("1999/photo.jpg", "sample.jpg");
    let engine = a.plan(&Recipe::default());
    engine
        .export(&AtomicBool::new(false), &mut |_| {})
        .unwrap_or_else(|error| panic!("{error:#}: {:?}", engine.items(0, None, 10).unwrap()));
    let item = engine.items(0, None, 10).unwrap().remove(0);
    let mut tool = MetadataTool::new().unwrap();
    for path in [
        PathBuf::from(&item.destination),
        Path::new(&item.destination).with_extension("xmp"),
    ] {
        assert!(metadata::keywords_from(&tool.read(&path).unwrap()).contains(&"1999".into()));
    }
}
#[test]
fn detected_content_type_controls_metadata_writes_and_appledouble_is_skipped() {
    let a = Archive::new();
    a.add("004. Selected Photos (Albums)/photo.jpg", "sample.png");
    a.add("._resource.jpg", "sample.jpg");
    let engine = a.plan(&Recipe::default());
    assert_eq!(engine.summary().unwrap()["files"], 1);
    engine
        .export(&AtomicBool::new(false), &mut |_| {})
        .unwrap_or_else(|error| panic!("{error:#}: {:?}", engine.items(0, None, 10).unwrap()));
    let item = engine.items(0, None, 10).unwrap().remove(0);
    assert_eq!(item.status, "done");
    let mut tool = MetadataTool::new().unwrap();
    assert!(metadata::keywords_from(
        &tool
            .read(&Path::new(&item.destination).with_extension("xmp"))
            .unwrap()
    )
    .contains(&"004. Selected Photos (Albums)".into()));
}
#[test]
fn metadata_matrix_exports_raw_tiff_dng_heic_png_video() {
    let a = Archive::new();
    for fixture in [
        "CanonRaw.cr2",
        "Nikon.nef",
        "DNG.dng",
        "sample.tiff",
        "sample.png",
        "QuickTime.mov",
        "QuickTime.heic",
    ] {
        a.add(fixture, fixture);
    }
    let e = a.plan(&Recipe {
        keywords: vec!["Matrix".into()],
        ..Recipe::default()
    });
    e.export(&AtomicBool::new(false), &mut |_| {})
        .unwrap_or_else(|err| panic!("{err:#}: {:?}", e.items(0, None, 100).unwrap()));
    for item in e.items(0, None, 100).unwrap() {
        assert_eq!(item.status, "done");
        if item.kind == "video" {
            assert!(item.destination.contains("/Video/"));
            assert_eq!(
                hash(Path::new(&item.source)),
                hash(Path::new(&item.destination))
            );
        } else {
            assert!(Path::new(&item.destination).with_extension("xmp").exists());
        }
    }
}
#[test]
fn exact_duplicates_merge_keywords_and_retain_other_sources_in_move_mode() {
    let a = Archive::new();
    let first = a.add("A/image.jpg", "sample.jpg");
    let second = a.add("B/image.jpg", "sample.jpg");
    let e = a.plan(&Recipe {
        operation: "move".into(),
        ..Recipe::default()
    });
    let rows = e.items(0, None, 10).unwrap();
    assert_eq!(rows.iter().filter(|i| i.status == "duplicate").count(), 1);
    let representative = rows.iter().find(|i| i.status == "planned").unwrap();
    assert!(representative.keywords.contains(&"A".into()));
    assert!(representative.keywords.contains(&"B".into()));
    e.export(&AtomicBool::new(false), &mut |_| {}).unwrap();
    assert_eq!(
        [first.exists(), second.exists()]
            .into_iter()
            .filter(|x| *x)
            .count(),
        1
    );
}
#[test]
fn conflicting_edit_sidecars_require_resolution() {
    let a = Archive::new();
    let first = a.add("A/raw.cr2", "CanonRaw.cr2");
    let second = a.add("B/raw.cr2", "CanonRaw.cr2");
    let mut tool = MetadataTool::new().unwrap();
    for (path, value) in [(&first, "1.0"), (&second, "2.0")] {
        tool.create_xmp(path, &path.with_extension("xmp")).unwrap();
        tool.execute(&[
            "-overwrite_original".into(),
            format!("-XMP-crs:Exposure2012={value}"),
            path.with_extension("xmp").to_str().unwrap().into(),
        ])
        .unwrap();
    }
    let e = a.plan(&Recipe::default());
    assert_eq!(e.summary().unwrap()["statuses"]["review"], 2);
    assert!(e.export(&AtomicBool::new(false), &mut |_| {}).is_err());
    let hash = e.items(0, None, 10).unwrap()[0].hash.clone().unwrap();
    e.resolve(&hash, "keep_all").unwrap();
    e.export(&AtomicBool::new(false), &mut |_| {}).unwrap();
    for item in e.items(0, None, 10).unwrap() {
        let meta = tool
            .read(&Path::new(&item.destination).with_extension("xmp"))
            .unwrap();
        let exposure = metadata::get(&meta, &["Exposure2012"]).unwrap();
        assert!(
            [1.0, 2.0].contains(&exposure.parse::<f64>().unwrap()),
            "{exposure}"
        );
    }
}
#[test]
fn shared_stems_and_existing_outputs_never_overwrite() {
    let a = Archive::new();
    a.add("image.jpg", "sample.jpg");
    a.add("image.png", "sample.png");
    fs::create_dir_all(a.output.join("Photos/Undated")).unwrap();
    let sentinel = a.output.join("Photos/Undated/image.xmp");
    fs::write(&sentinel, "do not overwrite").unwrap();
    let e = a.plan(&Recipe::default());
    let items = e.items(0, None, 10).unwrap();
    assert_ne!(
        Path::new(&items[0].destination).file_stem(),
        Path::new(&items[1].destination).file_stem()
    );
    e.export(&AtomicBool::new(false), &mut |_| {}).unwrap();
    assert_eq!(fs::read_to_string(sentinel).unwrap(), "do not overwrite");
}
#[test]
fn changed_source_is_retained_and_reported() {
    let a = Archive::new();
    let source = a.add("image.jpg", "sample.jpg");
    let e = a.plan(&Recipe {
        operation: "move".into(),
        ..Recipe::default()
    });
    fs::write(&source, "changed").unwrap();
    assert!(e.export(&AtomicBool::new(false), &mut |_| {}).is_err());
    assert!(source.exists());
    assert_eq!(e.summary().unwrap()["statuses"]["error"], 1);
}
#[test]
fn symlink_directories_and_nested_output_are_excluded() {
    let mut a = Archive::new();
    a.add("source.jpg", "sample.jpg");
    a.output = a.source.join("exports");
    fs::create_dir(&a.output).unwrap();
    fs::copy(a.source.join("source.jpg"), a.output.join("copied.jpg")).unwrap();
    std::os::unix::fs::symlink(&a.source, a.source.join("loop")).unwrap();
    let e = a.plan(&Recipe::default());
    assert_eq!(e.summary().unwrap()["files"], 1);
}
#[test]
fn destination_race_and_symlink_are_rejected() {
    let a = Archive::new();
    a.add("image.jpg", "sample.jpg");
    let e = a.plan(&Recipe {
        operation: "move".into(),
        ..Recipe::default()
    });
    let item = e.items(0, None, 1).unwrap().remove(0);
    let dest = Path::new(&item.destination);
    fs::create_dir_all(dest.parent().unwrap()).unwrap();
    fs::write(dest, "another user's output").unwrap();
    assert!(e.export(&AtomicBool::new(false), &mut |_| {}).is_err());
    assert_eq!(fs::read_to_string(dest).unwrap(), "another user's output");
    assert!(Path::new(&item.source).exists());
    fs::remove_file(dest).unwrap();
    e.export(&AtomicBool::new(false), &mut |_| {}).unwrap();
    assert!(!Path::new(&item.source).exists());
}
#[test]
fn cancellation_and_resume_are_idempotent() {
    let a = Archive::new();
    a.add("image.jpg", "sample.jpg");
    let e = a.plan(&Recipe::default());
    let cancel = AtomicBool::new(false);
    assert!(e
        .export(&cancel, &mut |_| { cancel.store(true, Ordering::Relaxed) })
        .is_err());
    drop(e);
    let e = a.engine();
    e.export(&AtomicBool::new(false), &mut |_| {}).unwrap();
    e.export(&AtomicBool::new(false), &mut |_| {}).unwrap();
    assert_eq!(e.summary().unwrap()["statuses"]["done"], 1);
}
#[test]
fn raw_move_reuses_inode_and_retains_raw_bytes() {
    use std::os::unix::fs::MetadataExt;
    let a = Archive::new();
    let source = a.add("raw.cr2", "CanonRaw.cr2");
    let before = hash(&source);
    let inode = fs::metadata(&source).unwrap().ino();
    let e = a.plan(&Recipe {
        operation: "move".into(),
        ..Recipe::default()
    });
    e.export(&AtomicBool::new(false), &mut |_| {}).unwrap();
    let item = e.items(0, None, 1).unwrap().remove(0);
    assert!(!source.exists());
    assert_eq!(hash(Path::new(&item.destination)), before);
    assert_eq!(fs::metadata(&item.destination).unwrap().ino(), inode);
}
#[test]
fn report_and_keyset_pagination_keep_all_source_references() {
    let a = Archive::new();
    a.add("one.jpg", "sample.jpg");
    a.add("two.jpg", "sample.jpg");
    let e = a.plan(&Recipe::default());
    let first = e.items(0, None, 1).unwrap();
    let second = e.items(first[0].id, None, 1).unwrap();
    assert_ne!(first[0].id, second[0].id);
    let mut report = vec![];
    e.report(&mut report).unwrap();
    let text = String::from_utf8(report).unwrap();
    assert!(text.contains("one.jpg"));
    assert!(text.contains("two.jpg"));
    assert_eq!(text.lines().count(), 3);
}
#[test]
fn scan_records_bad_media_without_aborting_archive() {
    let a = Archive::new();
    a.add("good.jpg", "sample.jpg");
    fs::write(a.source.join("broken.jpg"), b"not a jpeg").unwrap();
    let e = a.engine();
    e.scan(
        std::slice::from_ref(&a.source),
        &a.output,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(e.summary().unwrap()["scan_errors"], 1);
    assert_eq!(e.summary().unwrap()["files"], 1);
}

#[test]
fn skips_only_matched_web_variants_and_derives_event_names_offline() {
    let a = Archive::new();
    let low = fs::canonicalize(a.add(
        "Events/Wedding 2024/Web/portrait_instagram.jpg",
        "sample.jpg",
    ))
    .unwrap();
    let high = fs::canonicalize(a.add(
        "Events/Wedding 2024/High Res/portrait_Full.jpg",
        "sample.jpg",
    ))
    .unwrap();
    let lone =
        fs::canonicalize(a.add("Events/Wedding 2024/Web/speech_instagram.jpg", "sample.jpg"))
            .unwrap();
    for (path, suffix) in [(&high, b"high".as_slice()), (&lone, b"lone".as_slice())] {
        let mut bytes = fs::read(path).unwrap();
        bytes.extend_from_slice(suffix);
        fs::write(path, bytes).unwrap();
    }
    let engine = a.plan(&Recipe {
        operation: "move".into(),
        filename_template: "{event_name}_{stem}".into(),
        folder_tags: false,
        event_folder_tags: true,
        skip_lower_resolution_variants: true,
        ..Recipe::default()
    });
    let items = engine.items(0, None, 10).unwrap();
    let low_item = items
        .iter()
        .find(|item| item.source == low.to_string_lossy())
        .unwrap();
    let high_item = items
        .iter()
        .find(|item| item.source == high.to_string_lossy())
        .unwrap();
    let lone_item = items
        .iter()
        .find(|item| item.source == lone.to_string_lossy())
        .unwrap();
    assert_eq!(low_item.status, "skipped");
    assert_eq!(high_item.status, "planned");
    assert_eq!(lone_item.status, "planned");
    assert!(high_item.keywords.contains(&"Wedding 2024".into()));
    assert!(high_item
        .destination
        .ends_with("Wedding 2024_portrait_Full.jpg"));

    engine.export(&AtomicBool::new(false), &mut |_| {}).unwrap();
    assert!(
        low.exists(),
        "a skipped low-resolution source must remain in move mode"
    );
    assert!(!high.exists());
    assert!(!lone.exists());
}
