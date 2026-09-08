#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use photo_hat_engine::{Engine, Progress, Recipe};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{Manager, State};
use tauri_plugin_updater::UpdaterExt;

#[derive(Default, Clone, Serialize)]
struct Job {
    running: bool,
    progress: Option<Progress>,
    error: Option<String>,
}
struct AppState {
    db: Mutex<PathBuf>,
    job: Arc<Mutex<Job>>,
    cancel: Arc<AtomicBool>,
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn engine(state: &AppState) -> Result<Engine, String> {
    Engine::open(&state.db.lock().map_err(err)?).map_err(err)
}
#[tauri::command]
fn snapshot(state: State<AppState>) -> Result<Value, String> {
    Ok(
        json!({"job":state.job.lock().map_err(err)?.clone(),"summary":engine(&state)?.summary().map_err(err)?,"database":state.db.lock().map_err(err)?.display().to_string()}),
    )
}
#[tauri::command]
fn rows(
    state: State<AppState>,
    after: i64,
    status: Option<String>,
) -> Result<Vec<photo_hat_engine::Item>, String> {
    engine(&state)?
        .items(after, status.as_deref(), 100)
        .map_err(err)
}
#[tauri::command]
fn folder_candidates(state: State<AppState>) -> Result<Vec<String>, String> {
    engine(&state)?.folders().map_err(err)
}
#[tauri::command]
fn scan_errors(state: State<AppState>, after: String) -> Result<Vec<Value>, String> {
    engine(&state)?.scan_errors(&after).map_err(err)
}
#[tauri::command]
fn default_recipe() -> Recipe {
    Recipe::default()
}
#[tauri::command]
fn cancel(state: State<AppState>) {
    state.cancel.store(true, Ordering::Relaxed);
}
#[tauri::command]
fn new_session(app: tauri::AppHandle, state: State<AppState>) -> Result<(), String> {
    if state.job.lock().map_err(err)?.running {
        return Err("Wait for the current job to stop".into());
    }
    let path = app.path().app_data_dir().map_err(err)?.join(format!(
        "archive-{}.sqlite",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(err)?
            .as_millis()
    ));
    Engine::open(&path).map_err(err)?;
    std::fs::write(
        app.path().app_data_dir().map_err(err)?.join("last-session"),
        path.to_string_lossy().as_bytes(),
    )
    .map_err(err)?;
    *state.db.lock().map_err(err)? = path;
    *state.job.lock().map_err(err)? = Job::default();
    Ok(())
}
#[tauri::command]
fn open_session(app: tauri::AppHandle, state: State<AppState>, path: String) -> Result<(), String> {
    if state.job.lock().map_err(err)?.running {
        return Err("Wait for the current job to stop".into());
    }
    let path = PathBuf::from(path);
    if !path.is_file() {
        return Err("Session does not exist".into());
    }
    Engine::open(&path).map_err(err)?;
    std::fs::write(
        app.path().app_data_dir().map_err(err)?.join("last-session"),
        path.to_string_lossy().as_bytes(),
    )
    .map_err(err)?;
    *state.db.lock().map_err(err)? = path;
    *state.job.lock().map_err(err)? = Job::default();
    Ok(())
}
#[tauri::command]
fn start(
    state: State<AppState>,
    action: String,
    sources: Option<Vec<String>>,
    output: Option<String>,
    recipe: Option<Recipe>,
) -> Result<(), String> {
    let mut job = state.job.lock().map_err(err)?;
    if job.running {
        return Err("A job is already running".into());
    }
    if !["scan", "plan", "export", "resume"].contains(&action.as_str()) {
        return Err("Unknown action".into());
    }
    *job = Job {
        running: true,
        progress: None,
        error: None,
    };
    state.cancel.store(false, Ordering::Relaxed);
    let db = state.db.lock().map_err(err)?.clone();
    let job = state.job.clone();
    let cancel = state.cancel.clone();
    std::thread::spawn(move || {
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> anyhow::Result<()> {
                let engine = Engine::open(&db)?;
                let mut notify = |p| {
                    if let Ok(mut j) = job.lock() {
                        j.progress = Some(p);
                    }
                };
                match action.as_str() {
                    "scan" => engine.scan(
                        &sources
                            .unwrap_or_default()
                            .into_iter()
                            .map(PathBuf::from)
                            .collect::<Vec<_>>(),
                        &PathBuf::from(
                            output.ok_or_else(|| anyhow::anyhow!("Select an output folder"))?,
                        ),
                        &cancel,
                        &mut notify,
                    )?,
                    "plan" => engine.plan(&recipe.unwrap_or_default(), &cancel, &mut notify)?,
                    "resume" if engine.setting("plan_state")?.as_deref() != Some("ready") => {
                        let roots: Vec<PathBuf> = serde_json::from_str(
                            &engine
                                .setting("roots")?
                                .ok_or_else(|| anyhow::anyhow!("No scan to resume"))?,
                        )?;
                        engine.scan(
                            &roots,
                            &PathBuf::from(engine.setting("output")?.unwrap_or_default()),
                            &cancel,
                            &mut notify,
                        )?;
                    }
                    _ => engine.export(&cancel, &mut notify)?,
                }
                Ok(())
            }));
        if let Ok(mut j) = job.lock() {
            j.running = false;
            j.error = match result {
                Ok(Ok(())) => None,
                Ok(Err(e)) => Some(format!("{e:#}")),
                Err(_) => Some("Worker stopped unexpectedly; resume from saved state".into()),
            };
        }
    });
    Ok(())
}
#[tauri::command]
fn resolve(state: State<AppState>, hash: String, decision: String) -> Result<(), String> {
    if state.job.lock().map_err(err)?.running {
        return Err("Wait for the job to finish".into());
    }
    engine(&state)?.resolve(&hash, &decision).map_err(err)
}
#[tauri::command]
fn save_recipe(path: String, recipe: Recipe) -> Result<(), String> {
    recipe.validate().map_err(err)?;
    std::fs::write(path, serde_json::to_vec_pretty(&recipe).map_err(err)?).map_err(err)
}
#[tauri::command]
fn load_recipe(path: String) -> Result<Recipe, String> {
    let recipe: Recipe = serde_json::from_slice(&std::fs::read(path).map_err(err)?).map_err(err)?;
    recipe.validate().map_err(err)?;
    Ok(recipe)
}
#[tauri::command]
fn save_report(state: State<AppState>, path: String) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(err)?;
    engine(&state)?.report(&mut file).map_err(err)
}

#[derive(Default, Serialize, Deserialize)]
struct ReleaseConfig {
    endpoint: String,
    pubkey: String,
}
fn release_config() -> ReleaseConfig {
    serde_json::from_str(include_str!("../release.json")).unwrap_or_default()
}
#[tauri::command]
async fn check_update(app: tauri::AppHandle) -> Result<Value, String> {
    let config = release_config();
    if config.endpoint.is_empty() || config.pubkey.is_empty() {
        return Ok(json!({"configured":false}));
    }
    let updater = app
        .updater_builder()
        .endpoints(vec![config.endpoint.parse().map_err(err)?])
        .map_err(err)?
        .pubkey(config.pubkey)
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(err)?;
    match updater.check().await.map_err(err)? {
        Some(update) => Ok(json!({"configured":true,"version":update.version,"body":update.body})),
        None => Ok(json!({"configured":true,"version":null})),
    }
}
#[tauri::command]
async fn install_update(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    {
        let mut job = state.job.lock().map_err(err)?;
        if job.running {
            return Err("Finish the current job before updating".into());
        }
        job.running = true;
    }
    let result = async {
        let config = release_config();
        if config.endpoint.is_empty() || config.pubkey.is_empty() {
            return Err("Release signing is not configured".into());
        }
        let updater = app
            .updater_builder()
            .endpoints(vec![config.endpoint.parse().map_err(err)?])
            .map_err(err)?
            .pubkey(config.pubkey)
            .build()
            .map_err(err)?;
        if let Some(update) = updater.check().await.map_err(err)? {
            update
                .download_and_install(|_, _| {}, || {})
                .await
                .map_err(err)?;
            app.restart();
        }
        Ok(())
    }
    .await;
    state.job.lock().map_err(err)?.running = false;
    result
}
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let result: anyhow::Result<()> = (|| {
                let data = app.path().app_data_dir()?;
                std::fs::create_dir_all(&data)?;
                let path = std::fs::read_to_string(data.join("last-session"))
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| data.join("archive.sqlite"));
                let resources = app.path().resource_dir()?.join("resources/metadata");
                if resources.join("perl").exists() {
                    std::env::set_var("PHOTO_HAT_METADATA_DIR", resources);
                }
                Engine::open(&path)?;
                app.manage(AppState {
                    db: Mutex::new(path),
                    job: Arc::new(Mutex::new(Job::default())),
                    cancel: Arc::new(AtomicBool::new(false)),
                });
                Ok(())
            })();
            if let Err(error) = result {
                let _ = std::fs::write(
                    "/tmp/photo-hat-startup.log",
                    format!("Photo Sorting Hat startup failed:\n{error:#}\n"),
                );
                return Err(Box::<dyn std::error::Error>::from(error));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            snapshot,
            rows,
            folder_candidates,
            scan_errors,
            default_recipe,
            cancel,
            new_session,
            open_session,
            start,
            resolve,
            save_recipe,
            load_recipe,
            save_report,
            check_update,
            install_update
        ])
        .build(tauri::generate_context!())
        .expect("Failed to build Photo Sorting Hat")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                let state = app.state::<AppState>();
                if state.job.lock().map(|j| j.running).unwrap_or(true) {
                    api.prevent_exit();
                    state.cancel.store(true, Ordering::Relaxed);
                }
            }
        });
}
