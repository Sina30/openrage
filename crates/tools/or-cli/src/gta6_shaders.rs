//! `rpf scan-open` on GTA VI's shader bodies: an `SGD5` effect container's effects, parameters
//! and techniques by name and count, and the pipeline cache's header and record counts.

use std::collections::{BTreeMap, HashMap};

use or_io::gta5::shaders::pipeline_cache::{PipelineCache, PipelineCacheError};
use or_io::gta5::shaders::sgd5::{Sgd5Effect, Sgd5Error, Sgd5File, Sgd5Table, BANKS};

/// The head, the banks, the pieces not on disk and every effect of the container in `bytes`.
pub(crate) fn print_sgd5(bytes: &[u8]) -> Result<(), String> {
    let file = Sgd5File::open(bytes)
        .map_err(|e| format!("GTA VI's SGD5 reader does not read this body: {e}"))?;
    let g = &file.globals;
    println!(
        "  effects       SGD5, u64 {:#018X}, {} head layout(s), global table {} resources {} constants, pool {} B",
        file.head_word,
        file.layouts.len(),
        g.resources.len(),
        g.constants.len(),
        g.pool_len
    );
    for (i, l) in file.layouts.iter().enumerate() {
        println!(
            "    head layout {i:2} h64 {:016X} h32 {:08X} records {:3} classes {:?}",
            l.hash,
            l.id,
            l.bindings.len(),
            l.classes
        );
    }
    let mut names = HashMap::new();
    let mut located = [0usize; BANKS];
    for p in &file.programs {
        if let Some(bank) = p.bank {
            located[bank] += 1;
            if let Some(index) = p.index {
                names.insert((bank, index), p.name.as_str());
            }
        }
    }
    for (bank, count) in file.bank_counts.iter().enumerate() {
        let count = count.map_or("?".to_string(), |c| c.to_string());
        println!(
            "    bank {bank}: states {count} programs, {} located",
            located[bank]
        );
    }
    println!(
        "  programs      {} read, each a raw DEFLATE stream of its stated size; {}",
        file.programs.len(),
        Sgd5Error::NotTranslated
    );
    for gap in &file.gaps {
        match gap.resume {
            Some(at) => println!(
                "    piece not on disk: from +{:#X}, the read resumes at +{at:#X}",
                gap.from
            ),
            None => println!(
                "    piece not on disk: from +{:#X}, the effects after it are not read",
                gap.from
            ),
        }
    }
    let count = file.effect_count.map_or_else(
        || {
            format!(
                "{} read after the piece not on disk; the count is not on disk",
                file.effects.len()
            )
        },
        |c| c.to_string(),
    );
    println!("  effects       {count}; constants' defaults are not read, since where a default lies is not known");
    // Without a count, an effect is numbered from the end.
    let base = match file.effect_count {
        Some(_) => 0,
        None => -(file.effects.len() as i64),
    };
    for (i, e) in file.effects.iter().enumerate() {
        print_effect(base + i as i64, e, &names);
    }
    Ok(())
}

fn print_effect(label: i64, e: &Sgd5Effect, names: &HashMap<(usize, u32), &str>) {
    println!(
        "  effect {label} {} at +{:#X}, word {:#010X}",
        e.name, e.at, e.word
    );
    for (bank, list) in e.programs.iter().enumerate() {
        if list.is_empty() {
            continue;
        }
        let programs: Vec<String> = (list.iter())
            .map(|i| {
                names
                    .get(&(bank, *i))
                    .map_or(format!("#{i} not on disk"), |n| n.to_string())
            })
            .collect();
        println!(
            "    bank {bank}: {} program(s): {}",
            list.len(),
            programs.join(", ")
        );
    }
    let [rasterizer, depth, blend, sampler] = e.state_counts;
    println!("    states: rasterizer {rasterizer}, depth-stencil {depth}, blend {blend}, sampler {sampler}");
    let techniques: Vec<String> = (e.techniques.iter())
        .map(|t| format!("{}{}/{}", t.name, if t.flag { "*" } else { "" }, t.passes))
        .collect();
    println!(
        "    techniques {}: {}",
        techniques.len(),
        techniques.join(", ")
    );
    let passes: Vec<String> = (e.passes.iter())
        .map(|p| {
            let programs: Vec<String> = p.programs.iter().map(u8::to_string).collect();
            format!("{}[{}]", p.name, programs.join(" "))
        })
        .collect();
    println!("    passes {}: {}", passes.len(), passes.join(", "));
    for (pass, bound) in e.passes.iter().zip(&e.bound) {
        let [resources, unordered, empty, buffers] = bound.lists.each_ref().map(Vec::len);
        let trailing = bound
            .trailing
            .map_or(String::new(), |w| format!(", trailing word {w:#X}"));
        println!(
            "      {} binds {resources} shader resource(s), {unordered} unordered-access, {empty} in list 2, {buffers} constant buffer(s){trailing}",
            pass.name
        );
    }
    for (what, table) in [("parameters", &e.parameters), ("second table", &e.second)] {
        print_table(what, table);
    }
    let records: Vec<String> = e
        .layouts
        .iter()
        .map(|l| l.bindings.len().to_string())
        .collect();
    println!(
        "    layouts: {} inline ({} records), {} reference(s) to head layouts",
        e.layouts.len(),
        records.join(", "),
        e.layout_refs.len()
    );
}

/// A table's records, each by its name hash at +0x10 and then the hash at +0x14 a drawable states.
fn print_table(what: &str, t: &Sgd5Table) {
    println!(
        "    {what}: {} resources, {} constants, {} annotations, {} names in the pool",
        t.resources.len(),
        t.constants.len(),
        t.annotations.len(),
        t.names.len()
    );
    let name = |hash: u32| t.name_of(hash).unwrap_or("-").to_string();
    for r in &t.resources {
        println!(
            "      res {:08X} {:08X} {:<24} type {:#04X} flags {:#04X} slot {} count {} size {}",
            r.name_hash,
            r.semantic_hash,
            name(r.name_hash),
            r.code,
            r.flags,
            r.slot,
            r.members,
            r.buffer_size
        );
    }
    for c in &t.constants {
        let default = match &c.default {
            Some(words) => words
                .iter()
                .map(|w| format!("{w:08X}"))
                .collect::<Vec<_>>()
                .join(" "),
            None => "-".to_string(),
        };
        println!(
            "      con {:08X} {:08X} {:<24} type {:#04X} count {} offset {} default@{} {default}",
            c.name_hash,
            c.semantic_hash,
            name(c.name_hash),
            c.code,
            c.count,
            c.offset,
            c.default_offset
        );
    }
}

/// The header facts, the layout and the record counts of the pipeline cache in `bytes`.
pub(crate) fn print_pipeline_cache(bytes: &[u8]) -> Result<(), String> {
    let cache = PipelineCache::open(bytes)
        .map_err(|e| format!("GTA VI's pipeline cache reader does not read this body: {e}"))?;
    println!(
        "  pipelines     PS version {}, ident {:?}, build {:?}, key {:#018X}, stamp {:#018X}",
        cache.version, cache.ident, cache.build, cache.key, cache.stamp
    );
    for (name, at) in cache.sections {
        println!("    {name:<15} {at:#010X}");
    }
    let at = |name: &str| (cache.sections.iter().find(|s| s.0 == name)).map_or(0, |s| s.1);
    println!(
        "  records       {} (graphics {}, compute {})",
        cache.graphics.len() + cache.compute.len(),
        cache.graphics.len(),
        cache.compute.len()
    );
    println!(
        "  blobs         {} metadata ({} B), {} code ({} B to the last end); zero tail {} B",
        cache.metadata_blobs,
        at("code") - at("metadata"),
        cache.code_blobs,
        at("tail") - at("code"),
        cache.tail_len
    );
    let mut patterns: BTreeMap<String, usize> = BTreeMap::new();
    for g in &cache.graphics {
        *patterns.entry(g.pattern()).or_default() += 1;
    }
    let patterns: Vec<String> = patterns.iter().map(|(p, n)| format!("{p} {n}")).collect();
    println!("  slot patterns {}", patterns.join(", "));
    let roles: Vec<String> = cache
        .roles
        .iter()
        .map(|(r, n)| format!("{r} {n}"))
        .collect();
    println!(
        "  size rules    hold for every metadata blob: {}",
        roles.join(", ")
    );
    println!("  code          {}", PipelineCacheError::NativeCode);
    Ok(())
}
