mod cli;
mod commands;
mod config;
mod finalize;
mod paths;
mod recorder;
mod state;
mod target;
mod util;
mod webcam;

use clap::Parser;
use cli::{Cli, Command};
use paths::Paths;
use std::process::ExitCode;

fn run() -> anyhow::Result<u8> {
    let cli = Cli::parse();
    let paths = Paths::discover();

    match cli.command.unwrap_or(Command::Status { json: false }) {
        Command::Start(args) => {
            if !recorder::start(&paths, args)? {
                return Ok(1);
            }
        }
        Command::Toggle(args) => {
            if !recorder::toggle(&paths, args)? {
                return Ok(1);
            }
        }
        Command::Stop { discard } => recorder::stop(&paths, discard)?,
        Command::Pause => recorder::pause(&paths)?,
        Command::Resume => recorder::resume(&paths)?,
        Command::TogglePause => recorder::toggle_pause(&paths)?,
        Command::Status { json } => commands::status(&paths, json)?,
        Command::Recent { json, limit } => commands::recent(&paths, json, limit)?,
        Command::Slowmo { file, factor } => commands::slowmo(&paths, file, &factor)?,
        Command::Trash { file } => commands::trash(&paths, file)?,
        Command::Config(args) => config::run(&paths, args)?,
        Command::Preview { json, note_only } => commands::preview(&paths, json, note_only)?,
        Command::Output => commands::output(&paths),
        Command::Doctor => commands::doctor(&paths),
        Command::Finalize(args) => finalize::run_job(&paths, args)?,
    }
    Ok(0)
}

fn main() -> ExitCode {
    // A closed pipe (`reel doctor | head`) should end the process quietly the
    // way every other Unix CLI does, not panic inside stdio.
    // SAFETY: installing SIG_DFL for SIGPIPE has no memory-safety effects.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    match run() {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}
