//! `openrage-cli wardrobe dump`: the clothes dump of a clothing content, for the scripts of a server.

use std::collections::HashMap;
use std::path::Path;
use std::process::ExitCode;

use or_wardrobe::dump::{dump, JSON, LUA};
use or_wardrobe::Depth;

use crate::project::{parse, Failure};
use Failure::{Run, Usage};

pub(crate) fn dump_of(args: &[String]) -> Result<ExitCode, Failure> {
    let lua = args.iter().any(|a| a == "--lua");
    let rest: Vec<String> = args.iter().filter(|a| *a != "--lua").cloned().collect();
    let (positional, flags): (Vec<String>, HashMap<String, String>) =
        parse(&rest, &["--module", "--out"])?;
    let (project, modules) = crate::wardrobe::clothing_of(&positional, &flags)?;
    let [id] = modules.as_slice() else {
        return Err(Usage(format!(
            "the project holds {} clothing contents: name one with --module",
            modules.len()
        )));
    };
    let module = project
        .container
        .module(id)
        .expect("chosen from the container");
    let (wardrobe, planned) =
        or_wardrobe::planned_with_model(&project.folder(), module, Depth::Open)
            .map_err(|e| Run(format!("content `{id}`: {e}")))?;
    for problem in &planned.contribution.problems {
        eprintln!("warning: {problem}");
    }
    let made = dump(&wardrobe, &planned.allocation);
    match flags.get("--out") {
        Some(out) => {
            std::fs::create_dir_all(out).map_err(|e| Run(format!("{out}: {e}")))?;
            for (path, text) in [(JSON, made.json()), (LUA, made.lua())] {
                let name = path.rsplit('/').next().unwrap_or(path);
                let at = Path::new(out).join(name);
                std::fs::write(&at, text).map_err(|e| Run(format!("{}: {e}", at.display())))?;
                println!("wrote {}", at.display());
            }
        }
        None if lua => print!("{}", made.lua()),
        None => print!("{}", made.json()),
    }
    Ok(ExitCode::SUCCESS)
}
