//! `pavo` on the command line. The menu bar app drives this same binary
//! with `--json`, so anything the app can do, a script can too.

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;

use anyhow::{anyhow, ensure, Result};
use pavo_core::{actions_for, Event};
use serde_json::json;

const HELP: &str = "pavo — convert files, locally.

usage:
  pavo actions <files...>          what you can do with these files
  pavo run <action> <files...>     do it, e.g. `pavo run to:mp4 clip.mov`

flags:
  --json          one json object per line, for scripts and the menu bar app
  --watch-stdin   cancel when stdin closes (used by the menu bar app)
  -h, --help
  -V, --version
";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let json = take_flag(&mut args, "--json");
    let watch_stdin = take_flag(&mut args, "--watch-stdin");

    let result = match args.first().map(String::as_str) {
        None | Some("-h" | "--help" | "help") => {
            print!("{HELP}");
            Ok(())
        }
        Some("-V" | "--version") => {
            println!("pavo {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("actions") => actions(&args[1..], json),
        Some("run") => run(&args[1..], json, watch_stdin),
        Some(other) => Err(anyhow!("unknown command `{other}`, try `pavo --help`")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            if json {
                emit(json!({ "event": "error", "message": format!("{e:#}") }));
            } else {
                eprintln!("pavo: {e:#}");
            }
            ExitCode::FAILURE
        }
    }
}

fn take_flag(args: &mut Vec<String>, flag: &str) -> bool {
    let before = args.len();
    args.retain(|a| a != flag);
    args.len() != before
}

fn emit(value: serde_json::Value) {
    println!("{value}");
}

fn files(args: &[String]) -> Result<Vec<PathBuf>> {
    ensure!(!args.is_empty(), "no files given");
    args.iter()
        .map(|a| {
            let path = std::path::absolute(a)?;
            ensure!(path.exists(), "{a} doesn't exist");
            Ok(path)
        })
        .collect()
}

fn actions(args: &[String], json: bool) -> Result<()> {
    let actions = actions_for(&files(args)?);
    if json {
        let list: Vec<_> = actions
            .iter()
            .map(|a| json!({ "id": a.id, "label": a.label, "group": a.group.as_str() }))
            .collect();
        emit(json!({ "actions": list }));
    } else if actions.is_empty() {
        println!("nothing to do with these files yet");
    } else {
        for a in &actions {
            println!("  {:<16} {}", a.id, a.label);
        }
    }
    Ok(())
}

fn run(args: &[String], json: bool, watch_stdin: bool) -> Result<()> {
    ensure!(args.len() >= 2, "usage: pavo run <action> <files...>");
    let action = &args[0];
    let files = files(&args[1..])?;

    // stay out of the way of whatever else the computer is doing
    // SAFETY: plain syscall on our own process
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 10);
    }

    if watch_stdin {
        // the app holds our stdin open; if it closes (cancel, quit or crash), stop
        thread::spawn(|| {
            let mut stdin = std::io::stdin();
            let mut buf = [0u8; 64];
            while matches!(stdin.read(&mut buf), Ok(n) if n > 0) {}
            pavo_core::cancel();
        });
    }

    let (mut index, mut total, mut last) = (0, files.len(), -1);
    let outputs = pavo_core::run(action, &files, &mut |event| match event {
        Event::Start { input, index: i, total: t } => {
            (index, total) = (i, t);
            if !json {
                eprintln!("{} ({}/{t})", input.display(), i + 1);
            }
        }
        Event::Progress(f) => {
            // progress across all the files, not just the current one
            let overall = (index as f64 + f) / total as f64;
            let pct = (overall * 100.0) as i32;
            if pct != last {
                last = pct;
                if json {
                    emit(json!({ "event": "progress", "fraction": (overall * 1000.0).round() / 1000.0 }));
                } else {
                    eprint!("\r  {pct:>3}%");
                }
            }
        }
        Event::Output(path) => {
            if json {
                emit(json!({ "event": "output", "path": path }));
            } else {
                eprintln!("\r  → {}", path.display());
            }
        }
    })?;

    if json {
        emit(json!({ "event": "done", "outputs": outputs }));
    }
    Ok(())
}
