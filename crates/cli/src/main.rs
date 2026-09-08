use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use photo_hat_engine::{Engine, Recipe};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[derive(Parser)]
#[command(
    version,
    about = "Offline photo and video organization with verified exports"
)]
struct Cli {
    #[arg(long, default_value = "photo-hat.sqlite", global = true)]
    db: PathBuf,
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Action,
}
#[derive(Subcommand)]
enum Action {
    Scan {
        #[arg(required = true)]
        sources: Vec<PathBuf>,
        #[arg(long)]
        output: PathBuf,
    },
    Plan {
        #[arg(long)]
        recipe: Option<PathBuf>,
        #[arg(long)]
        move_files: bool,
    },
    Export,
    Resume,
    Status,
    List {
        #[arg(long, default_value_t = 0)]
        after: i64,
        #[arg(long)]
        status: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: u32,
    },
    Resolve {
        hash: String,
        #[arg(value_parser=["keep_all","use_first"])]
        decision: String,
    },
    Report {
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Recipe,
    Formats,
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{}", serde_json::json!({"error":format!("{e:#}")}));
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let cli = Cli::parse();
    // A packaged CLI finds the metadata runtime beside its executable.
    if std::env::var_os("PHOTO_HAT_METADATA_DIR").is_none() {
        if let Some(root) = std::env::current_exe()?.parent() {
            let metadata = root.join("metadata");
            if metadata.join("perl").exists() {
                std::env::set_var("PHOTO_HAT_METADATA_DIR", metadata);
            }
        }
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = cancel.clone();
    ctrlc::set_handler(move || {
        signal.store(true, Ordering::Relaxed);
    })?;
    let mut notify = |p: photo_hat_engine::Progress| {
        if cli.json {
            eprintln!("{}", serde_json::to_string(&p).unwrap());
        } else {
            eprintln!("{}: {} / {} {}", p.phase, p.completed, p.total, p.current);
        }
    };
    if matches!(cli.command, Action::Recipe) {
        println!("{}", serde_json::to_string_pretty(&Recipe::default())?);
        return Ok(());
    }
    if matches!(cli.command, Action::Formats) {
        println!(
            "{}",
            serde_json::to_string_pretty(&photo_hat_engine::metadata::capabilities()?)?
        );
        return Ok(());
    }
    let engine = Engine::open(&cli.db)?;
    match cli.command {
        Action::Scan { sources, output } => engine.scan(&sources, &output, &cancel, &mut notify)?,
        Action::Plan { recipe, move_files } => {
            let mut recipe: Recipe = match recipe {
                Some(path) => serde_json::from_slice(&std::fs::read(path)?)?,
                None => Recipe::default(),
            };
            if move_files {
                recipe.operation = "move".into();
            }
            engine.plan(&recipe, &cancel, &mut notify)?;
        }
        Action::Export => engine.export(&cancel, &mut notify)?,
        Action::Resume => {
            if engine.setting("plan_state")?.as_deref() == Some("ready") {
                engine.export(&cancel, &mut notify)?;
            } else {
                let sources: Vec<PathBuf> =
                    serde_json::from_str(&engine.setting("roots")?.context("No scan to resume")?)?;
                let output = PathBuf::from(engine.setting("output")?.context("Missing output")?);
                engine.scan(&sources, &output, &cancel, &mut notify)?;
            }
        }
        Action::Status => {}
        Action::List {
            after,
            status,
            limit,
        } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&engine.items(after, status.as_deref(), limit)?)?
            );
            return Ok(());
        }
        Action::Resolve { hash, decision } => engine.resolve(&hash, &decision)?,
        Action::Report { output } => {
            match output {
                Some(path) => engine.report(
                    &mut std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(path)?,
                )?,
                None => engine.report(&mut std::io::stdout())?,
            };
            return Ok(());
        }
        Action::Recipe | Action::Formats => unreachable!(),
    }
    println!("{}", serde_json::to_string_pretty(&engine.summary()?)?);
    Ok(())
}
