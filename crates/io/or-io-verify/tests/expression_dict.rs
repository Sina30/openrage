//! `.yed` through its declarations, over the corpus: typed reads against the
//! tree, the recorded testimony re-keyed by offset, the canonical fixed point
//! and the alignment census.
//!
//! ⚠ A program's three runs are one `Trailing` whose length is the sum of the
//! three header fields that count them.

mod declared;

use std::collections::BTreeMap;

use declared::{file_tree, files, moved_by, name, Filed, Opened};
use or_io::decl::{Plain, Raw, Record, View};
use or_io::desc::{Image, Pack};
use or_io::gta5::expression_dict::*;
use or_io::Generation;
use or_io_verify::census;

const EXT: &str = "yed";
const T: Generation = Generation::Gen8;

type E = or_io::decl::ViewError;

#[derive(Default)]
struct Reached(BTreeMap<(usize, usize), Vec<u8>>);

impl Reached {
    fn put<R: Record>(&mut self, v: &View<'_, R>) {
        let bytes = Plain::as_bytes(View::get(v)).to_vec();
        self.0.insert((View::offset(v), bytes.len()), bytes);
    }
    fn run<R: Record>(&mut self, run: or_io::decl::Run<'_, R>) {
        for v in run.views() {
            self.put(&v);
        }
    }
}

#[derive(Default)]
struct Counts {
    expressions: usize,
    programs: usize,
    code_bytes: usize,
    tracks: usize,
    springs: usize,
    variables: usize,
}

fn typed(image: Image<'_>, out: &mut Reached, n: &mut Counts) -> Result<(), E> {
    let dict = View::<Raw<CrExpressionsDictionary>>::at_offset(image, 0)?;
    out.put(&dict);
    let entries = dict.entries()?;
    for i in 0..entries.len() {
        let Some(expression) = entries.follow(i)? else {
            continue;
        };
        n.expressions += 1;
        out.put(&expression);
        let programs = expression.expression_streams()?;
        for p in 0..programs.len() {
            let Some(program) = programs.follow(p)? else {
                continue;
            };
            n.programs += 1;
            n.code_bytes += program.data()?.len();
            out.put(&program);
        }
        let (tracks, springs) = (expression.tracks()?, expression.motions()?);
        n.tracks += tracks.len();
        n.springs += springs.len();
        n.variables += expression.variables()?.len();
        out.run(tracks);
        out.run(springs);
    }
    Ok(())
}

#[test]
fn every_yed_reads_typed_as_the_tree_reads() {
    let Some(paths) = files(EXT, Some(T), "a typed .yed read against the tree") else {
        return;
    };
    let (mut same, mut n, mut failures) = (0usize, Counts::default(), Vec::new());
    for path in &paths {
        let opened = Opened::read(path);
        let image = opened.image();
        let tree = declared::format(EXT).read(T, image).expect("reads");
        let mut filed = Filed::default();
        file_tree(image, &tree, Some(0), &mut filed);
        let from_tree: BTreeMap<(usize, usize), Vec<u8>> = filed
            .structs
            .into_iter()
            .map(|((at, _), b)| ((at, b.len()), b))
            .collect();
        let mut reached = Reached::default();
        if let Err(e) = typed(image, &mut reached, &mut n) {
            failures.push(format!("{}: {e}", name(path)));
            continue;
        }
        for (key, bytes) in &reached.0 {
            match from_tree.get(key) {
                Some(t) if t == bytes => same += 1,
                Some(_) => failures.push(format!("{}: {key:x?} reads differently", name(path))),
                None => failures.push(format!(
                    "{}: the tree holds nothing at {key:x?}",
                    name(path)
                )),
            }
        }
    }
    println!(
        "{} .yed: {} expressions, {} programs holding {} code bytes, {} tracks, {} springs, \
         {} variables; {same} typed instances read as the tree reads them",
        paths.len(),
        n.expressions,
        n.programs,
        n.code_bytes,
        n.tracks,
        n.springs,
        n.variables
    );
    assert!(same > 0 && n.programs > 0);
    failures.truncate(20);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A program's `code` is the sum of its three counts, so a typed read of it
/// ends exactly where the next allocation may begin.
#[test]
fn every_expression_states_its_longest_program() {
    let Some(paths) = files(EXT, Some(T), "each expression's stated scratch size") else {
        return;
    };
    let (mut exact, mut expressions, mut failures) = (0usize, 0usize, Vec::new());
    for path in &paths {
        let opened = Opened::read(path);
        let dict = View::<Raw<CrExpressionsDictionary>>::at_offset(opened.image(), 0).unwrap();
        let entries = dict.entries().unwrap();
        for i in 0..entries.len() {
            let Some(expression) = entries.follow(i).unwrap() else {
                continue;
            };
            expressions += 1;
            let longest = longest_program(&expression).unwrap();
            let programs = expression.expression_streams().unwrap();
            for p in 0..programs.len() {
                let program = programs.follow(p).unwrap().unwrap();
                assert_eq!(
                    program.data().unwrap().len() + 0x10,
                    0x10 + program.align_param_size as usize
                        + program.param_size as usize
                        + usize::from(program.num_operations)
                );
            }
            if longest == expression.max_packed_size as usize {
                exact += 1;
            } else {
                failures.push(format!(
                    "{}: expression {i} states {} and holds {longest}",
                    name(path),
                    expression.max_packed_size
                ));
            }
        }
    }
    println!("{exact} of {expressions} expressions state their longest program exactly");
    failures.truncate(20);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(exact > 0);
}

#[test]
fn every_yed_opens_owned_and_saves_as_its_tree_saves() {
    let Some(paths) = files(EXT, Some(T), "the owned .yed against the tree") else {
        return;
    };
    let got = declared::owned_saves_as_tree::<CrExpressionsDictionary>(EXT, T, &paths);
    println!(
        ".yed: {} files, {} opened owned, saved byte-identical to the tree's save and at a \
         fixed point, {} refused by both",
        got.files,
        got.same,
        got.refused.len()
    );
    assert!(got.failures.is_empty(), "{}", got.failures.join("\n"));
    assert!(got.refused.is_empty(), "{:?}", got.refused);
    assert_eq!(got.same, paths.len());
}

#[test]
fn every_yed_saves_to_a_fixed_point() {
    let Some(paths) = files(EXT, Some(T), "the .yed fixed point") else {
        return;
    };
    let got = declared::fixed_point(EXT, T, &paths, &[]);
    println!(
        ".yed: {} file(s), {} at a fixed point",
        got.files, got.fixed
    );
    assert!(got.failures.is_empty(), "{}", got.failures.join("\n"));
    assert_eq!(got.fixed, got.files);
}

/// An expression whose scratch is one byte short of its longest program.
#[test]
fn an_expression_whose_scratch_is_short_is_refused() {
    let Some(paths) = files(EXT, Some(T), "a broken .yed refused") else {
        return;
    };
    let yed = declared::format(EXT);
    let opened = Opened::read(&paths[0]);
    let dict = View::<Raw<CrExpressionsDictionary>>::at_offset(opened.image(), 0).unwrap();
    let first = dict.entries().unwrap().follow(0).unwrap().unwrap();
    let at = View::offset(&first) + 0x78;
    let short = first.max_packed_size - 1;
    let mut bytes = opened.bytes.bytes().to_vec();
    bytes[at..at + 4].copy_from_slice(&short.to_le_bytes());
    let aligned = or_io::decl::Aligned::from_bytes(&bytes);
    let tree = yed
        .read(T, Image::new(aligned.bytes(), opened.virtual_size))
        .expect("reads");
    let why = yed
        .save(T, &tree)
        .expect_err("a program outgrows its scratch")
        .to_string();
    println!("{}: {why}", name(&paths[0]));
    assert!(why.contains("would overrun it"), "{why}");
}

#[test]
fn every_yed_instance_is_aligned_for_its_view() {
    let Some(paths) = files(EXT, Some(T), "the .yed alignment census") else {
        return;
    };
    let pack = declared::pack(EXT, T);
    let root = declared::format(EXT).root_name(T).unwrap();
    let mut total = census::Census::default();
    for path in &paths {
        total.fold(census::take(&pack, Opened::read(path).image(), &root, 0));
    }
    for (name, tally) in &total.types {
        println!(
            "{name:<22} {:>8} instances, {} misaligned, alignment {}",
            tally.instances, tally.misaligned, tally.alignment
        );
    }
    assert!(total.unreached.is_empty(), "{:?}", total.unreached);
    let misaligned: Vec<_> = total.misaligned().collect();
    assert!(misaligned.is_empty(), "{misaligned:?}");
}

#[test]
#[ignore = "a timing, meaningful only in a release build; run by name"]
fn what_a_yed_read_costs() {
    use std::hint::black_box;
    use std::time::{Duration, Instant};
    let Some(paths) = files(EXT, Some(T), "the .yed read cost") else {
        return;
    };
    let opened: Vec<Opened> = paths.iter().map(|p| Opened::read(p)).collect();
    let best = |rounds: usize, f: &mut dyn FnMut() -> usize| {
        let mut best = Duration::MAX;
        for _ in 0..5 {
            let start = Instant::now();
            let mut sink = 0usize;
            for _ in 0..rounds {
                sink = sink.wrapping_add(f());
            }
            black_box(sink);
            best = best.min(start.elapsed());
        }
        best.as_secs_f64() * 1e3 / rounds as f64
    };
    // Every track's bone tag and every program's opcode count, typed from the root.
    #[inline(never)]
    fn typed_sum(image: Image<'_>) -> usize {
        let dict = View::<Raw<CrExpressionsDictionary>>::at_offset(image, 0).unwrap();
        let entries = dict.entries().unwrap();
        let mut total = 0usize;
        for e in (0..entries.len()).filter_map(|i| entries.follow(i).ok().flatten()) {
            total += e
                .tracks()
                .unwrap()
                .iter()
                .map(|t| usize::from(t.id))
                .sum::<usize>();
            let programs = e.expression_streams().unwrap();
            for p in (0..programs.len()).filter_map(|i| programs.follow(i).ok().flatten()) {
                total += usize::from(p.num_operations);
            }
        }
        total
    }
    let typed_ms = best(200, &mut || {
        opened.iter().map(|o| typed_sum(black_box(o.image()))).sum()
    });
    let tree_ms = best(3, &mut || {
        opened
            .iter()
            .map(|o| {
                usize::from(
                    declared::format(EXT)
                        .read(T, o.image())
                        .unwrap()
                        .record()
                        .is_some(),
                )
            })
            .sum()
    });
    println!(
        "{} files: every track's bone and every program's opcode count typed from the root \
         {typed_ms:.4} ms; the whole tree read {tree_ms:.2} ms ({:.0}x)",
        paths.len(),
        tree_ms / typed_ms
    );
}

fn builtin(t: Generation) -> Option<Pack> {
    (t == T).then(|| (*declared::pack(EXT, T)).clone())
}

#[test]
fn the_yed_testimony_holds_through_its_declarations() {
    let Some(rows) = declared::check_against(
        EXT,
        &builtin,
        &[],
        &[],
        "the .yed testimony through the declarations",
    ) else {
        return;
    };
    let (total, mut failures) = declared::judge(&rows);
    assert!(total > 0, "no recorded field was asked");
    failures.truncate(30);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_declared_field_one_byte_late_is_named_by_the_testimony() {
    let late = moved_by(&declared::pack(EXT, T), "Track", "track", 1);
    let Some(rows) = declared::caught_against(
        EXT,
        &|t| (t == T).then(|| late.clone()),
        &[],
        &[],
        "a misdeclared .yed against the testimony",
    ) else {
        return;
    };
    for (label, t) in &rows {
        let caught = t.differ.len() + t.unanswered;
        println!("{label}: {caught} caught with a `Track` track one byte late");
        assert!(caught > 0, "{label} did not notice");
    }
}
