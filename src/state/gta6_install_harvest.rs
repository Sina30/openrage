//! Every GTA VI archive of an install read once for its textures, as a job after the install's
//! scan: each texture dictionary's keys, so a drawable's key opens a dictionary whose archive no
//! one opened, and the names the naming rules give from each drawable's names. What each archive
//! gave is kept in the configuration folder and served at the next start; an archive is read again
//! only once its size, its modification time or the scan's rules change.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use or_io::container::rpf::{
    read_rpf9_body, seal_rpf9_scan, unseal_rpf9_scan, Rpf9BodyKind, Rpf9ZlibRun,
};
use or_io::gta6::hash::name_hash64;
use or_io::gta6::headerless::{Entry, Resource};
use or_io::gta6::texture_names::name_keys;

use super::gta6_dictionary_index::Built;
use super::gta6_install_scan::{lock, Hit, InstallFound};
use super::gta6_texture_index::{put, stat, stated};
use super::gta6_texture_naming::{NamingEnded, TextureNaming};
use super::rpf9_kept::{word, RULES};
use super::rpf9_scan::{body_path, Ended, RowKind, View};
use super::AppState;

/// The version of what this file keeps: a change to it takes another.
const VERSION: u64 = 2;
/// The most a kept harvest inflates to.
const MOST_BYTES: usize = 1 << 28;
/// The most one body read for the harvest inflates to.
const BODY_LIMIT: u64 = 1 << 30;

/// The install's dictionaries as the harvest found them, and whether the kept harvest was read this session.
#[derive(Default)]
pub(super) struct Harvest {
    pub(super) built: Arc<Mutex<Option<Built>>>,
    read: bool,
}

/// What one archive's bodies gave while its size and modification time are `stat` and the pack
/// keys its names were matched against digest to `keys`: each dictionary by the path it opens
/// under with its textures' keys, and each name with its key.
#[derive(Debug, Default, Clone, PartialEq)]
struct Harvested {
    stat: (u64, u64),
    keys: u64,
    dictionaries: Vec<(PathBuf, Vec<u64>)>,
    names: Vec<(u64, String)>,
}

/// A digest of a set of pack keys, which changes when a key is added or taken away.
fn digest(keys: &HashSet<u64>) -> u64 {
    let mix = |key: &u64| key.wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(29);
    (keys.iter().map(mix)).fold(keys.len() as u64, u64::wrapping_add)
}

/// Whether the harvest reads the body of `hit`: a dictionary, or a drawable the model view draws.
fn read_for_textures(hit: &Hit) -> bool {
    matches!(
        (hit.kind, hit.view),
        (RowKind::Dictionary, Some(View::Facts(_))) | (RowKind::Model, Some(View::Drawable))
    )
}

/// The resource the body at `offset` of `file` reads to; `None` where it does not read.
fn body(file: &File, offset: u64) -> Option<Resource> {
    let (find, bytes) = read_rpf9_body(file, offset, BODY_LIMIT).ok()?;
    let Rpf9BodyKind::Zlib(Rpf9ZlibRun {
        resource: Some(resource),
        signature: Some(signature),
        ..
    }) = find.kind
    else {
        return None;
    };
    let (virtual_section, physical) = resource.sections(&bytes);
    or_io::gta6::headerless::read(signature, virtual_section, Some(physical)).ok()
}

/// The keys of the textures that read in `resource` where it is a dictionary; into `names`, each
/// texture's name and each name the rules give from its drawables' names for a key of `keys`, or
/// of the keys the drawables bind where `keys` is empty.
fn gathered(
    resource: &Resource,
    keys: &HashSet<u64>,
    names: &mut HashMap<u64, String>,
) -> Vec<u64> {
    let mut held = Vec::new();
    if let Resource::Dictionary(dict) = resource {
        for (_, entry) in &dict.entries {
            if let Ok(Entry::Texture(t)) = entry {
                let key = name_hash64(t.name.as_bytes());
                names.entry(key).or_insert_with(|| t.name.clone());
                if t.texture.is_ok() {
                    held.push(key);
                }
            }
        }
    }
    let stems = super::ttpf_document::stated_names(resource);
    if stems.is_empty() {
        return held;
    }
    let bound: HashSet<u64>;
    let keys = match keys.is_empty() {
        false => keys,
        true => {
            let drawables = super::gta6_resource::dictionary_drawables(resource)
                .map(|(_, d)| d)
                .chain(super::gta6_resource::drawable_of(resource, None));
            let bound_keys = drawables
                .flat_map(|d| &d.shaders)
                .flat_map(|s| &s.texture_keys);
            bound = bound_keys.flatten().copied().collect();
            &bound
        }
    };
    for (key, (name, _)) in name_keys(stems, keys) {
        names.entry(key).or_insert(name);
    }
    held
}

/// Each archive of `archives` that `kept` does not hold at its size and time as they are now and
/// for these `keys`, its bodies of the hits beside it read and gathered for `keys`, and how many
/// bodies read; with whether `cancel` stopped it, the archives read before the stop given.
fn harvest(
    archives: Vec<(PathBuf, Vec<Hit>)>,
    kept: &HashMap<PathBuf, Harvested>,
    keys: &HashSet<u64>,
    cancel: &or_job::Cancel,
    progress: &or_job::Progress,
) -> (Vec<(PathBuf, Harvested)>, usize, bool) {
    let stamp = digest(keys);
    let todo: Vec<(PathBuf, (u64, u64), Vec<Hit>)> = (archives.into_iter())
        .filter_map(|(path, hits)| {
            let now = stat(&path)?;
            let unchanged = kept
                .get(&path)
                .is_some_and(|k| k.stat == now && k.keys == stamp);
            (!unchanged).then_some((path, now, hits))
        })
        .collect();
    let total: u64 = todo.iter().map(|(.., hits)| hits.len() as u64).sum();
    let (mut out, mut done, mut read) = (Vec::new(), 0u64, 0usize);
    for (path, now, hits) in todo {
        let Ok(file) = File::open(&path) else {
            continue;
        };
        let mut names = HashMap::new();
        let mut harvested = Harvested {
            stat: now,
            keys: stamp,
            ..Harvested::default()
        };
        for hit in hits {
            if cancel.is_set() {
                return (out, read, true);
            }
            progress.reached(done, total);
            done += 1;
            let (Some(view), Some(resource)) = (hit.view, body(&file, hit.offset)) else {
                continue;
            };
            read += 1;
            let held = gathered(&resource, keys, &mut names);
            if !held.is_empty() {
                let at = body_path(&path, hit.offset, view);
                harvested.dictionaries.push((at, held));
            }
        }
        harvested.names = names.into_iter().collect();
        out.push((path, harvested));
    }
    progress.reached(total, total);
    (out, read, false)
}

/// Serve through `built` the dictionaries of each archive of `kept` whose size and time stand,
/// and learn the names their bodies gave; how many keys were named.
fn serve(kept: &HashMap<PathBuf, Harvested>, built: &Mutex<Option<Built>>) -> usize {
    let standing: Vec<&Harvested> = (kept.iter())
        .filter(|(path, harvested)| stat(path) == Some(harvested.stat))
        .map(|(_, harvested)| harvested)
        .collect();
    let names = standing.iter().flat_map(|h| h.names.clone());
    let named = super::ttpf_document::learn_named(names);
    let dictionaries = standing.iter().flat_map(|h| h.dictionaries.clone());
    *built.lock().unwrap_or_else(|p| p.into_inner()) = Some(Built::of(dictionaries));
    named
}

/// Where the harvest is kept, one file for every install.
// ponytail: an archive since moved or deleted stays in it, as an archive's kept scan does.
fn kept_file() -> Option<PathBuf> {
    let root = super::prefs::config_root()?;
    Some(root.join("gta6_harvest.kept"))
}

/// What a kept harvest starts with: this file's version, then the rules of the scans whose bodies it read.
fn head() -> Vec<u8> {
    let words = [VERSION, RULES];
    words.into_iter().flat_map(u64::to_le_bytes).collect()
}

fn encode(kept: &HashMap<PathBuf, Harvested>) -> Vec<u8> {
    let mut plain = head();
    for (archive, harvested) in kept {
        put(&mut plain, archive.to_string_lossy().as_bytes());
        let (len, modified) = harvested.stat;
        let counted = harvested.dictionaries.len() as u64;
        for word in [len, modified, harvested.keys, counted] {
            plain.extend(word.to_le_bytes());
        }
        for (body, keys) in &harvested.dictionaries {
            put(&mut plain, body.to_string_lossy().as_bytes());
            let words: Vec<u8> = keys.iter().flat_map(|key| key.to_le_bytes()).collect();
            put(&mut plain, &words);
        }
        plain.extend((harvested.names.len() as u64).to_le_bytes());
        for (key, name) in &harvested.names {
            plain.extend(key.to_le_bytes());
            put(&mut plain, name.as_bytes());
        }
    }
    plain
}

/// The path the next length in `bytes` states, which moves past it.
fn path_in(bytes: &mut &[u8]) -> io::Result<PathBuf> {
    let path = String::from_utf8_lossy(stated(bytes)?).into_owned();
    Ok(PathBuf::from(path))
}

/// What [`encode`] wrote, sealed; an error for another version or other rules, and for bytes cut short or altered.
fn decode(sealed: &[u8]) -> io::Result<HashMap<PathBuf, Harvested>> {
    let plain = unseal_rpf9_scan(sealed, MOST_BYTES)?;
    let other = || {
        let why = "a kept harvest of another version, of other scan rules, or altered";
        io::Error::new(io::ErrorKind::InvalidData, why)
    };
    let bytes = &mut (plain.strip_prefix(head().as_slice())).ok_or_else(other)?;
    let mut kept = HashMap::new();
    while !bytes.is_empty() {
        let archive = path_in(bytes)?;
        let mut harvested = Harvested {
            stat: (word(bytes)?, word(bytes)?),
            keys: word(bytes)?,
            ..Harvested::default()
        };
        for _ in 0..word(bytes)? {
            let body = path_in(bytes)?;
            let (keys, rest) = stated(bytes)?.as_chunks::<8>();
            if !rest.is_empty() {
                return Err(other());
            }
            let keys = keys.iter().map(|key| u64::from_le_bytes(*key)).collect();
            harvested.dictionaries.push((body, keys));
        }
        for _ in 0..word(bytes)? {
            let key = word(bytes)?;
            let name = stated(bytes)?.to_vec();
            let name = String::from_utf8(name).map_err(|_| other())?;
            harvested.names.push((key, name));
        }
        kept.insert(archive, harvested);
    }
    Ok(kept)
}

/// The harvest kept at `file`; none where none is kept, or it does not read.
fn read_kept(file: &Path) -> HashMap<PathBuf, Harvested> {
    let Ok(sealed) = std::fs::read(file) else {
        return HashMap::new();
    };
    decode(&sealed)
        .inspect_err(|e| tracing::info!("{}: {e}", file.display()))
        .unwrap_or_default()
}

/// Keep `kept` at `file`, in place of the harvest kept before.
fn keep(file: &Path, kept: &HashMap<PathBuf, Harvested>) -> io::Result<()> {
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)?;
    }
    std::fs::write(file, seal_rpf9_scan(&encode(kept))?)
}

impl AppState {
    /// Serve the harvest kept in the configuration folder, read on a worker once a session.
    pub(super) fn read_kept_harvest(&mut self) {
        let harvest = &mut self.texture_packs.dictionaries.harvest;
        if std::mem::replace(&mut harvest.read, true) {
            return;
        }
        let Some(file) = kept_file().filter(|file| file.is_file()) else {
            return;
        };
        let built = Arc::clone(&harvest.built);
        std::thread::spawn(move || serve(&read_kept(&file), &built));
    }

    /// Read, as a job, the dictionaries and drawables of each archive under `root` that the
    /// install scan `found` read whole, an archive the kept harvest holds as it is now excepted.
    pub(super) fn harvest_install(&mut self, root: &Path, found: &Mutex<InstallFound>) {
        let archives: Vec<(PathBuf, Vec<Hit>)> = {
            let found = lock(found);
            (found.archives.iter())
                .filter(|archive| archive.ended == Ended::Whole)
                .map(|archive| {
                    let hits = archive.hits.iter().copied().filter(read_for_textures);
                    (archive.path.clone(), hits.collect())
                })
                .collect()
        };
        if archives.iter().all(|(_, hits)| hits.is_empty()) {
            return;
        }
        let Some(file) = kept_file() else {
            return;
        };
        let packs = super::gta6_texture_index::pack_keys(root);
        let built = Arc::clone(&self.texture_packs.dictionaries.harvest.built);
        let doing = tr!(
            "Reading the drawables and texture dictionaries of every GTA VI archive under {folder}",
            folder = root.display()
        );
        let run = TextureNaming::run(doing, move |cancel, progress| {
            let Some(keys) = packs(cancel, progress) else {
                return NamingEnded::Stopped;
            };
            let mut kept = read_kept(&file);
            let (harvested, read, stopped) = harvest(archives, &kept, &keys, cancel, progress);
            let changed = !harvested.is_empty();
            kept.extend(harvested);
            if let Some(Err(error)) = changed.then(|| keep(&file, &kept)) {
                tracing::warn!(%error, "the harvest of the install's archives was not kept");
            }
            let named = serve(&kept, &built);
            match stopped {
                true => NamingEnded::Stopped,
                false => NamingEnded::Harvested(read, named),
            }
        });
        self.texture_naming.push(run);
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use or_io::gta6::headerless::{Drawable, Shader};

    use super::*;
    use crate::state::gta6_edit::tests::dictionary_file;
    use crate::state::rpf9_scan::view_of;

    /// A dictionary resource file in `dir`, the hit of its one body, and the key of its one texture.
    fn dictionary(dir: &Path) -> (PathBuf, Hit, u64) {
        let (path, _) = dictionary_file(dir);
        let file = File::open(&path).expect("opened");
        let (find, _) = read_rpf9_body(&file, 0, BODY_LIMIT).expect("a body");
        let hit = Hit {
            offset: 0,
            kind: RowKind::Dictionary,
            view: view_of(&find).ok(),
        };
        assert!(read_for_textures(&hit), "{hit:?}");
        (path, hit, name_hash64(b"zq_paint"))
    }

    #[test]
    fn an_archive_is_read_once_for_its_dictionaries_and_names_and_again_once_it_changes() {
        let dir = tempfile::tempdir().expect("a scratch directory");
        let (archive, hit, key) = dictionary(dir.path());
        let none = HashSet::new();
        let (cancel, progress) = (or_job::Cancel::new(), or_job::Progress::new());
        let archives = || vec![(archive.clone(), vec![hit])];
        let (read, bodies, stopped) =
            harvest(archives(), &HashMap::new(), &none, &cancel, &progress);
        assert_eq!((bodies, stopped), (1, false));
        let view = hit.view.expect("a view");
        let wanted = Harvested {
            stat: stat(&archive).expect("there"),
            keys: digest(&none),
            dictionaries: vec![(body_path(&archive, 0, view), vec![key])],
            names: vec![(key, "zq_paint".to_string())],
        };
        assert_eq!(read, [(archive.clone(), wanted.clone())]);

        let kept: HashMap<PathBuf, Harvested> = read.into_iter().collect();
        let sealed = seal_rpf9_scan(&encode(&kept)).expect("sealed");
        assert_eq!(decode(&sealed).expect("decoded"), kept);
        let (read, bodies, _) = harvest(archives(), &kept, &none, &cancel, &progress);
        assert!(
            read.is_empty() && bodies == 0,
            "an archive kept as it is was read"
        );
        // A pack added since the archive was kept gives keys its names were not matched against.
        let packs = HashSet::from([key ^ 1]);
        let (_, bodies, _) = harvest(archives(), &kept, &packs, &cancel, &progress);
        assert_eq!(
            bodies, 1,
            "an archive kept for other pack keys was not read"
        );

        let modified = std::fs::metadata(&archive).and_then(|m| m.modified());
        let later = modified.expect("a time") + Duration::from_secs(10);
        let file = File::options().write(true).open(&archive).expect("opened");
        file.set_modified(later).expect("timed");
        drop(file);
        let (read, bodies, _) = harvest(archives(), &kept, &none, &cancel, &progress);
        assert_eq!(bodies, 1, "an archive written again was not read");
        assert_eq!(read[0].1.dictionaries, wanted.dictionaries);

        let built = Mutex::new(None);
        serve(&kept, &built);
        let stale = served(&built, key);
        assert!(
            stale.is_none(),
            "an archive written since it was kept is served"
        );
        let again: HashMap<PathBuf, Harvested> = read.into_iter().collect();
        serve(&again, &built);
        let body = wanted.dictionaries[0].0.clone();
        assert_eq!(served(&built, key), Some(body));

        cancel.set();
        let (read, bodies, stopped) =
            harvest(archives(), &HashMap::new(), &none, &cancel, &progress);
        assert!(read.is_empty() && bodies == 0 && stopped);
    }

    /// The body the index `built` serves `key` from.
    fn served(built: &Mutex<Option<Built>>, key: u64) -> Option<PathBuf> {
        let built = built.lock().expect("unlocked");
        built.as_ref()?.body_of(key).cloned()
    }

    #[test]
    fn a_kept_harvest_cut_short_or_of_other_scan_rules_does_not_read() {
        let kept = HashMap::from([(
            PathBuf::from("a.rpf"),
            Harvested {
                stat: (1, 2),
                keys: 3,
                dictionaries: vec![(PathBuf::from("a.rpf/0x40.rsc"), vec![7, 8])],
                names: vec![(9, "zq_a".to_string())],
            },
        )]);
        let plain = encode(&kept);
        let sealed = |plain: &[u8]| seal_rpf9_scan(plain).expect("sealed");
        assert_eq!(decode(&sealed(&plain)).expect("read"), kept);
        for cut in 0..plain.len() {
            let read = decode(&sealed(&plain[..cut]));
            assert!(
                read.is_err() || cut == head().len(),
                "{cut} bytes read as {read:?}"
            );
        }
        // The scan's rules are the second word a kept harvest starts with.
        let mut other = plain.clone();
        other[8] ^= 1;
        assert!(decode(&sealed(&other)).is_err());
    }

    #[test]
    fn a_drawable_s_name_names_the_keys_it_binds_or_else_the_keys_the_packs_hold() {
        let texture = name_hash64(b"zq_harvest_07_nm");
        let drawable = Drawable {
            name: "zq_harvest_07.#dr".to_string(),
            box_min: [0.0; 3],
            box_max: [1.0; 3],
            shaders: vec![Shader {
                name_hash: 1,
                texture_keys: vec![Some(texture)],
                parameters: Vec::new(),
            }],
            levels: [None, None, None, None],
            lod_distances: [10.0; 4],
            bound: None,
            embedded: None,
            extra_skeletons: Vec::new(),
        };
        let resource = Resource::Drawable(drawable);
        let mut names = HashMap::new();
        let held = gathered(&resource, &HashSet::new(), &mut names);
        assert!(held.is_empty(), "a drawable is no dictionary");
        let wanted = HashMap::from([(texture, "zq_harvest_07_nm".to_string())]);
        assert_eq!(names, wanted);
        let mut names = HashMap::new();
        gathered(&resource, &HashSet::from([texture ^ 1]), &mut names);
        assert!(names.is_empty(), "a key no pack holds was named: {names:?}");
    }

    #[test]
    fn a_kept_harvest_serves_its_dictionaries_and_names_at_the_next_start() {
        let config = tempfile::tempdir().expect("a configuration folder");
        let dir = tempfile::tempdir().expect("a scratch directory");
        let (archive, hit, key) = dictionary(dir.path());
        crate::state::prefs::keep_prefs_in(config.path());
        let (cancel, progress) = (or_job::Cancel::new(), or_job::Progress::new());
        let archives = vec![(archive, vec![hit])];
        let (mut read, ..) = harvest(
            archives,
            &HashMap::new(),
            &HashSet::new(),
            &cancel,
            &progress,
        );
        // A name only this kept file holds: the fixture's own name is learned by other tests of
        // this process too.
        let only_kept = name_hash64(b"zq_kept_harvest_only");
        let only = (only_kept, "zq_kept_harvest_only".to_string());
        read[0].1.names.push(only);
        let file = kept_file().expect("a configuration folder");
        keep(&file, &read.into_iter().collect()).expect("kept");

        let mut state = AppState::new();
        assert!(
            !state.texture_packs.dictionaries.holds(key),
            "served before it was read"
        );
        state.read_kept_harvest();
        let until = Instant::now() + Duration::from_secs(30);
        while !state.texture_packs.dictionaries.holds(key) {
            assert!(Instant::now() < until, "the kept harvest was never served");
            std::thread::sleep(Duration::from_millis(5));
        }
        // The index is served after the names are learned.
        let name = crate::state::ttpf_document::known_name(only_kept);
        assert_eq!(name.as_deref(), Some("zq_kept_harvest_only"));
    }
}
