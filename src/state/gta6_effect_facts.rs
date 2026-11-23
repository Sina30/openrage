//! A GTA VI `SGD5` effect container opened read-only, as GTA V's `.fxc` opens: the lines its
//! reader reads, its banks and programs, then each effect's programs, techniques, passes and tables.

use std::collections::HashMap;
use std::path::Path;

use or_io::gta5::shaders::sgd5::{Sgd5Effect, Sgd5File, Sgd5Table, BANKS};

use super::document::DocPayload;

/// The container in `bytes` as its lines; the job its row starts learns its effects for the
/// drawables' shaders.
pub(super) fn payload(path: &Path, bytes: &[u8]) -> Result<DocPayload, String> {
    let file = Sgd5File::open(bytes)
        .map_err(|e| tr!("GTA VI's SGD5 reader does not read this body: {e}", e = e))?;
    let text = lines(&file).join("\n");
    Ok(super::document_io::text_payload(path, text.into_bytes()))
}

fn or_unknown<T: ToString>(value: Option<T>) -> String {
    value.map_or_else(|| "?".to_string(), |v| v.to_string())
}

/// The head, each bank, each piece not on disk and each program, then every effect.
fn lines(file: &Sgd5File) -> Vec<String> {
    let g = &file.globals;
    let mut lines = vec![tr!(
        "A GTA VI SGD5 effect container: head word {word}, {layouts} head layouts and a global table of {resources} resources and {constants} constants. It is read-only, as GTA V's effects are, so Save As writes a copy of the stored body.",
        word = format!("{:#018X}", file.head_word),
        layouts = file.layouts.len(),
        resources = g.resources.len(),
        constants = g.constants.len(),
    )];
    let mut names = HashMap::new();
    let mut located = [0usize; BANKS];
    for p in &file.programs {
        let Some(bank) = p.bank else { continue };
        if let Some(count) = located.get_mut(bank) {
            *count += 1;
        }
        if let Some(index) = p.index {
            names.insert((bank, index), p.name.as_str());
        }
    }
    for (bank, (count, read)) in file.bank_counts.iter().zip(located).enumerate() {
        lines.push(tr!(
            "Bank {bank}: {count} programs stated, {read} read.",
            bank = bank,
            count = or_unknown(*count),
            read = read,
        ));
    }
    for gap in &file.gaps {
        let from = format!("+{:#X}", gap.from);
        lines.push(match gap.resume {
            Some(at) => tr!(
                "A piece of the file a download has not delivered starts at {from}; the read resumes at {at}.",
                from = from,
                at = format!("+{at:#X}"),
            ),
            None => tr!(
                "A piece of the file a download has not delivered starts at {from}; nothing after it is read.",
                from = from,
            ),
        });
    }
    lines.push(tr!(
        "Programs: {count} read, each a raw DEFLATE stream. None is translated here: a drawable drawn Shaded with one of these effects names why its programs are refused.",
        count = file.programs.len(),
    ));
    for p in &file.programs {
        lines.push(tr!(
            "Program {name}: bank {bank}, index {index}, kind {kind}, key {key}, a stream of {stream} bytes that inflates to {inflated}.",
            name = p.name,
            bank = or_unknown(p.bank),
            index = or_unknown(p.index),
            kind = p.kind,
            key = format!("{:016X}", p.key),
            stream = p.stream.len(),
            inflated = p.inflated_len,
        ));
    }
    lines.push(match file.effect_count {
        Some(count) => tr!("Effects: {count}.", count = count),
        None => tr!(
            "Effects: {count} read after the last piece not delivered; the count is not on disk.",
            count = file.effects.len(),
        ),
    });
    for e in &file.effects {
        effect_lines(e, &names, &mut lines);
    }
    lines
}

fn effect_lines(e: &Sgd5Effect, names: &HashMap<(usize, u32), &str>, lines: &mut Vec<String>) {
    let [rasterizer, depth, blend, sampler] = e.state_counts;
    lines.push(String::new());
    lines.push(tr!(
        "Effect {name} at {at}, word {word}: rasterizer, depth-stencil, blend and sampler states {rasterizer}, {depth}, {blend} and {sampler}.",
        name = e.name,
        at = format!("+{:#X}", e.at),
        word = format!("{:#010X}", e.word),
        rasterizer = rasterizer,
        depth = depth,
        blend = blend,
        sampler = sampler,
    ));
    for (bank, list) in e.programs.iter().enumerate() {
        if list.is_empty() {
            continue;
        }
        let programs: Vec<String> = (list.iter())
            .map(|i| {
                names
                    .get(&(bank, *i))
                    .map_or_else(|| format!("#{i}"), |n| n.to_string())
            })
            .collect();
        let line = tr!(
            "Bank {bank}: {programs}.",
            bank = bank,
            programs = programs.join(", ")
        );
        lines.push(format!("  {line}"));
    }
    for t in &e.techniques {
        let line = tr!(
            "Technique {name}: {passes} passes, flag {flag}.",
            name = t.name,
            passes = t.passes,
            flag = u8::from(t.flag),
        );
        lines.push(format!("  {line}"));
    }
    for (i, p) in e.passes.iter().enumerate() {
        let programs: Vec<String> = p.programs.iter().map(u8::to_string).collect();
        let [resources, unordered, _, buffers] =
            (e.bound.get(i)).map_or([0; 4], |b| b.lists.each_ref().map(Vec::len));
        let line = tr!(
            "Pass {name}: programs {programs}; it binds {resources} shader resources, {unordered} unordered-access resources and {buffers} constant buffers.",
            name = p.name,
            programs = programs.join(" "),
            resources = resources,
            unordered = unordered,
            buffers = buffers,
        );
        lines.push(format!("  {line}"));
    }
    let (parameters, second) = (&e.parameters, &e.second);
    let head = tr!(
        "Parameters: {resources} resources, {constants} constants and {annotations} annotations.",
        resources = parameters.resources.len(),
        constants = parameters.constants.len(),
        annotations = parameters.annotations.len(),
    );
    table_lines(&head, parameters, lines);
    let head = tr!(
        "Second table: {resources} resources, {constants} constants and {annotations} annotations.",
        resources = second.resources.len(),
        constants = second.constants.len(),
        annotations = second.annotations.len(),
    );
    table_lines(&head, second, lines);
    let line = tr!(
        "Layouts: {inline} of its own and {references} references to head layouts.",
        inline = e.layouts.len(),
        references = e.layout_refs.len(),
    );
    lines.push(format!("  {line}"));
}

/// A table's records, each by its name where its pool holds it, else its hash.
fn table_lines(head: &str, t: &Sgd5Table, lines: &mut Vec<String>) {
    lines.push(format!("  {head}"));
    let name =
        |hash: u32| (t.name_of(hash)).map_or_else(|| format!("hash_{hash:08X}"), str::to_string);
    for r in &t.resources {
        let line = tr!(
            "Resource {name}: semantic {semantic}, type {code}, flags {flags}, slot {slot}, members {members}, size {size}.",
            name = name(r.name_hash),
            semantic = format!("{:08X}", r.semantic_hash),
            code = format!("{:#04X}", r.code),
            flags = format!("{:#04X}", r.flags),
            slot = r.slot,
            members = r.members,
            size = r.buffer_size,
        );
        lines.push(format!("    {line}"));
    }
    for c in &t.constants {
        let default = (c.default.as_ref()).map_or_else(
            || "-".to_string(),
            |words| {
                let words: Vec<String> = words.iter().map(|w| format!("{w:08X}")).collect();
                words.join(" ")
            },
        );
        let line = tr!(
            "Constant {name}: semantic {semantic}, type {code}, count {count}, offset {offset}, default {default}.",
            name = name(c.name_hash),
            semantic = format!("{:08X}", c.semantic_hash),
            code = format!("{:#04X}", c.code),
            count = c.count,
            offset = c.offset,
            default = default,
        );
        lines.push(format!("    {line}"));
    }
}

#[cfg(test)]
mod tests {
    use or_io::gta5::shaders::sgd5::{Sgd5Gap, Sgd5Pass, Sgd5Program, Sgd5Technique};

    use super::*;

    /// A container of one effect whose pass runs bank 0's program 0, and a piece not on disk.
    fn file() -> Sgd5File {
        let program = Sgd5Program {
            bank: Some(0),
            index: Some(0),
            name: "vs_main".into(),
            kind: 2,
            key: 0x0123_4567_89AB_CDEF,
            at: 0x40,
            stream: 0x44..0x54,
            inflated_len: 0x80,
            variables: Sgd5Table::default(),
        };
        let effect = Sgd5Effect {
            name: "standard_t".into(),
            at: 0x100,
            word: 7,
            programs: [vec![0, 3], vec![], vec![], vec![], vec![], vec![]],
            state_counts: [1, 2, 3, 4],
            techniques: vec![Sgd5Technique {
                name: "draw".into(),
                flag: true,
                index: 0,
                passes: 1,
            }],
            passes: vec![Sgd5Pass {
                name: "p0".into(),
                programs: [1, 0, 0, 0, 0, 0],
            }],
            bound: Vec::new(),
            parameters: Sgd5Table::default(),
            second: Sgd5Table::default(),
            layout_flag: 0,
            layout_refs: vec![5],
            layouts: Vec::new(),
        };
        Sgd5File {
            head_word: 0,
            layouts: Vec::new(),
            globals: Sgd5Table::default(),
            bank_counts: [Some(4), None, Some(0), Some(0), Some(0), Some(0)],
            programs: vec![program],
            gaps: vec![Sgd5Gap {
                from: 0x60,
                resume: None,
            }],
            effect_count: None,
            effects: vec![effect],
        }
    }

    #[test]
    fn a_container_lists_its_banks_programs_and_each_effect_s_parts() {
        let text = lines(&file()).join("\n");
        for said in [
            "Bank 0: 4 programs stated, 1 read.",
            "Bank 1: ? programs stated, 0 read.",
            "starts at +0x60; nothing after it is read.",
            "Program vs_main: bank 0, index 0, kind 2, key 0123456789ABCDEF, a stream of 16 bytes that inflates to 128.",
            "Effects: 1 read after the last piece not delivered",
            "Effect standard_t at +0x100, word 0x00000007: rasterizer, depth-stencil, blend and sampler states 1, 2, 3 and 4.",
            // A program not read is named by its index.
            "  Bank 0: vs_main, #3.",
            "  Technique draw: 1 passes, flag 1.",
            "  Pass p0: programs 1 0 0 0 0 0; it binds 0 shader resources",
            "  Layouts: 0 of its own and 1 references to head layouts.",
        ] {
            assert!(text.contains(said), "{said}\n{text}");
        }
    }
}
