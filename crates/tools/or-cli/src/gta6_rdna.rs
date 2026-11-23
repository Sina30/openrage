//! `rpf scan-rdna`: every program of a GTA VI effect container through the native-code front end,
//! as counts by stage and by refusal, and on request one program's disassembly.

use std::collections::BTreeMap;
use std::fs;

use or_io::container::rpf::{read_rpf9_body, Rpf9BodyKind};
use or_io::gta5::shaders::sgd5::Sgd5File;
use or_shader_front::rdna::{self, Cfg, Program};

/// How one stage's programs came out.
#[derive(Debug, Default, PartialEq, Eq)]
struct Stage {
    programs: usize,
    /// Those whose control flow follows from the first word to an end, every word an instruction.
    graphs: usize,
    translated: usize,
    /// Translated modules naga's validator refuses: a fault of the lowering, never of the program.
    invalid: usize,
    refused: BTreeMap<String, usize>,
}

/// What one program came to, as a line.
fn outcome(program: &Program, stage: &mut Stage) -> String {
    stage.programs += 1;
    let cfg = match Cfg::build(&program.words) {
        Ok(cfg) => cfg,
        Err(e) => {
            *stage.refused.entry("graph".to_owned()).or_default() += 1;
            return format!("no graph: {e}");
        }
    };
    stage.graphs += 1;
    match rdna::lower(program, &cfg) {
        Ok(lowered) => match rdna::check(&lowered.module) {
            Ok(()) => {
                stage.translated += 1;
                format!(
                    "translated: {} resource(s), {} approximation(s)",
                    lowered.resources.len(),
                    lowered.approximations.len()
                )
            }
            Err(why) => {
                stage.invalid += 1;
                format!("translated to a module naga refuses: {why}")
            }
        },
        Err(e) => {
            *stage.refused.entry(e.name()).or_default() += 1;
            format!("not translated: {e}")
        }
    }
}

/// The `--program <n>` a census takes, and its other words.
fn split_program(args: &[String]) -> Result<(Option<usize>, Vec<String>), String> {
    match args.iter().position(|a| a == "--program") {
        Some(i) => {
            let word = args.get(i + 1).ok_or("--program needs an index")?;
            let index = (word.parse()).map_err(|e| format!("{word} is not an index: {e}"))?;
            let mut rest = args.to_vec();
            rest.drain(i..i + 2);
            Ok((Some(index), rest))
        }
        None => Ok((None, args.to_vec())),
    }
}

/// The refusals of a stage printed at most, the commonest first.
const TOP_REFUSALS: usize = 12;

/// `rpf scan-rdna`: every program of the `SGD5` container at an offset decoded, its control flow
/// followed and its lowering tried; `--program` also prints that program's instructions.
pub(crate) fn cmd_scan_rdna(args: &[String]) -> Result<(), String> {
    let (limit, args) = super::rpf::split_limit(args)?;
    let (show, args) = split_program(&args)?;
    let [path, offset] = args.as_slice() else {
        return Err(
            "usage: rpf scan-rdna <archive.rpf> <offset> [--program <n>] [--limit <bytes>]"
                .to_string(),
        );
    };
    let offset = super::rpf::parse_offset(offset)?;
    let file = fs::File::open(path).map_err(|e| format!("open({path}): {e}"))?;
    let (find, bytes) = read_rpf9_body(&file, offset, limit).map_err(|e| e.to_string())?;
    if !matches!(find.kind, Rpf9BodyKind::Sgd5 { .. }) {
        return Err(format!(
            "the body at {offset:#X} is not an SGD5 effect container"
        ));
    }
    let container = Sgd5File::open(&bytes)
        .map_err(|e| format!("the SGD5 container at {offset:#X} does not read: {e}"))?;
    let mut stages: BTreeMap<String, Stage> = BTreeMap::new();
    let mut unread = 0usize;
    for (index, program) in container.programs.iter().enumerate() {
        let blocks = (bytes.get(program.stream.clone()))
            .and_then(|stream| program.inflate(stream).ok())
            .and_then(|dxbc| rdna::unwrap(&dxbc).ok());
        let Some(blocks) = blocks else {
            unread += 1;
            continue;
        };
        for block in &blocks {
            let key = format!("{:?} (slot {})", block.kind, block.slot).to_lowercase();
            let line = outcome(block, stages.entry(key.clone()).or_default());
            if show == Some(index) {
                println!(
                    "program {index}, {key}, {} words, {} lanes: {line}",
                    block.words.len(),
                    if block.wave32 { 32 } else { 64 }
                );
                // Follows no control flow: data after the last instruction prints as what its words decode to.
                for (pc, inst) in rdna::decode(&block.words).map_err(|e| e.to_string())? {
                    println!("  {pc:6}  {inst}");
                }
            }
        }
    }
    println!(
        "  programs      {} in the container, {unread} not read",
        container.programs.len()
    );
    for (key, stage) in &stages {
        let refused: usize = stage.refused.values().sum();
        println!(
            "  {key:<22} {}: {} with a graph, {} translated, {} translated but invalid, {refused} refused",
            stage.programs, stage.graphs, stage.translated, stage.invalid
        );
        let mut reasons: Vec<_> = stage.refused.iter().collect();
        reasons.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (reason, count) in reasons.iter().take(TOP_REFUSALS) {
            println!("    refused {reason}: {count}");
        }
        if reasons.len() > TOP_REFUSALS {
            println!("    and {} other reason(s)", reasons.len() - TOP_REFUSALS);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_shader_front::dxil::ShaderKind;

    fn words(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| (*a).to_owned()).collect()
    }

    #[test]
    fn the_program_flag_is_taken_out_of_the_words() {
        let (show, rest) = split_program(&words(&["a.rpf", "--program", "7", "0x10"])).unwrap();
        assert_eq!((show, rest), (Some(7), words(&["a.rpf", "0x10"])));
        assert!(split_program(&words(&["--program"])).is_err());
        assert!(split_program(&words(&["--program", "x"])).is_err());
    }

    #[test]
    fn an_outcome_is_counted_by_what_stopped_it() {
        let program = |kind, words| Program {
            kind,
            slot: 0,
            words,
            wave32: false,
            inputs: Vec::new(),
            vertex_inputs: Vec::new(),
            position_read: 0,
            front_face: false,
        };
        const S_ENDPGM: u32 = 0xBF81_0000;
        let mut stage = Stage::default();
        // A pixel program that only ends translates; a word of no family gives no graph;
        // a compute program is refused by its stage.
        outcome(&program(ShaderKind::Pixel, vec![S_ENDPGM]), &mut stage);
        outcome(&program(ShaderKind::Pixel, vec![0xC000_0000]), &mut stage);
        outcome(&program(ShaderKind::Compute, vec![S_ENDPGM]), &mut stage);
        assert_eq!(
            stage,
            Stage {
                programs: 3,
                graphs: 2,
                translated: 1,
                invalid: 0,
                refused: BTreeMap::from([("compute stage".to_owned(), 1), ("graph".to_owned(), 1)]),
            }
        );
    }
}
