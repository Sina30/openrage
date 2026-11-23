//! `openrage-cli photo`: the editor's renderer draws the pictures, so this starts the `openrage`
//! beside this binary as `openrage --photo`, which opens no window.

use std::process::{Command, ExitCode};

pub fn run(args: &[String]) -> ExitCode {
    let editor = match std::env::current_exe() {
        Ok(exe) => exe.with_file_name(format!("openrage{}", std::env::consts::EXE_SUFFIX)),
        Err(e) => {
            eprintln!("Error: cannot find this binary's folder: {e}");
            return ExitCode::from(1);
        }
    };
    if !editor.is_file() {
        eprintln!(
            "Error: photo draws with the editor, and {} is not there",
            editor.display()
        );
        return ExitCode::from(1);
    }
    match Command::new(&editor).arg("--photo").args(args).status() {
        Ok(status) => ExitCode::from(status.code().map_or(1, |code| code as u8)),
        Err(e) => {
            eprintln!("Error: cannot start {}: {e}", editor.display());
            ExitCode::from(1)
        }
    }
}
