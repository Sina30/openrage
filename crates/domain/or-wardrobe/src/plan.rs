//! The planner: what a pack hands to the plan of a build.
//!
//! [`plan`] is a pure function of the model and what the disk said of its files. [`contribution`]
//! reads both for a module of a project and hands the result over as the [`Contribution`] the plan
//! merges, so the clothing is one more kind of content going through the one path.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use or_io::decl::File;
use or_io::meta::{
    CAlternateVariations, CCreatureMetaData, CPedVariationInfo, FirstPersonAlternateData,
};
use or_project::{
    Claim, Container, Contribution, Figure, Item, ManifestEntry, Module, ModuleKind, Origin,
    Output, OutputKind, PlannedFile,
};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::alloc::{allocate, Allocation, Placed};
use crate::facts::{inspect, Depth, Fact, Facts};
use crate::follow::Led;
use crate::layout::{self, Paths};
use crate::metadata;
use crate::model::{self, Garment, Wardrobe};
use crate::names::Ped;
use crate::options::{DumpKind, Options};
use crate::rules::{check, Finding};
use crate::shop;

/// The kind of name the game resolves once that a pack's collections are.
pub const COLLECTION: &str = "clothing collection";

/// The metadata the plan writes, as the typed records it was made from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Documents {
    /// The variation metadata of the pack's own collection of each ped.
    pub variations: BTreeMap<Ped, CPedVariationInfo>,
    pub creature: Option<CCreatureMetaData>,
    /// The same for the collections a slot past its cap is cut into (`<collection>_1` and on), by ped and part.
    pub later_variations: BTreeMap<(Ped, u32), CPedVariationInfo>,
    pub later_shops: BTreeMap<(Ped, u32), String>,
    /// The creature metadata of each of those collections, by part.
    pub later_creatures: BTreeMap<u32, CCreatureMetaData>,
    /// The alternate variations of the pack, none where no set has a switch that something causes.
    pub alternates: Option<CAlternateVariations>,
    /// The first person alternates of the pack, none where no garment states one.
    pub first_person: Option<FirstPersonAlternateData>,
    pub shops: BTreeMap<Ped, String>,
}

impl Documents {
    /// The variation metadata of collection `part` of `ped`, none where it holds no garment.
    pub fn variation(&self, ped: Ped, part: u32) -> Option<&CPedVariationInfo> {
        match part {
            0 => self.variations.get(&ped),
            n => self.later_variations.get(&(ped, n)),
        }
    }

    /// The shop metadata of collection `part` of `ped`.
    pub fn shop(&self, ped: Ped, part: u32) -> Option<&String> {
        match part {
            0 => self.shops.get(&ped),
            n => self.later_shops.get(&(ped, n)),
        }
    }

    /// The creature metadata of collection `part`.
    pub fn creature_of(&self, part: u32) -> Option<&CCreatureMetaData> {
        match part {
            0 => self.creature.as_ref(),
            n => self.later_creatures.get(&n),
        }
    }

    /// Every collection that registers a shop file, the pack's own first: the ped and the part.
    pub fn collections(&self) -> Vec<(Ped, u32)> {
        let mut all: Vec<(Ped, u32)> = self.shops.keys().map(|p| (*p, 0)).collect();
        all.extend(self.later_shops.keys().copied());
        all.sort_by_key(|(ped, part)| (*part, *ped));
        all
    }

    fn put_variation(&mut self, (ped, part): (Ped, u32), info: CPedVariationInfo) {
        match part {
            0 => self.variations.insert(ped, info),
            n => self.later_variations.insert((ped, n), info),
        };
    }

    fn put_shop(&mut self, (ped, part): (Ped, u32), text: String) {
        match part {
            0 => self.shops.insert(ped, text),
            n => self.later_shops.insert((ped, n), text),
        };
    }
}

/// A pack planned.
#[derive(Debug, Clone)]
pub struct Planned {
    pub contribution: Contribution,
    pub allocation: Allocation,
    pub findings: Vec<Finding>,
    /// What the files say of each garment that a query asks of, in the pack's order.
    pub counts: Vec<crate::query::Counts>,
    /// Each file of the contribution that a garment's own files make, by its path, with the index of the garment: what a size map ranks by.
    pub owners: Vec<(String, usize)>,
    /// The files and sources the garments name, once each, and where each leads.
    pub dependencies: Vec<crate::sizes::Dependency>,
    pub documents: Documents,
    /// How the build it was planned for writes the files, which a verifier reads them back by.
    pub build: Build,
}

/// How one build writes a pack: where its files go, what its options leave out, and whether the files are those of a pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    pub paths: Paths,
    pub options: Options,
    pub kind: OutputKind,
}

impl Build {
    /// A FiveM resource with the default options, which is how a pack was planned before a build had options.
    pub fn standard(wardrobe: &Wardrobe) -> Self {
        Self {
            paths: Paths::resource(&wardrobe.pack, true),
            options: Options::default(),
            kind: OutputKind::FivemResource,
        }
    }

    /// The build `output` is, and a sentence for each of its options that cannot be used.
    pub fn of(wardrobe: &Wardrobe, output: &Output) -> (Self, Vec<String>) {
        let (options, problems) = Options::of(output);
        let paths = match output.kind {
            OutputKind::SingleplayerDlc | OutputKind::OivPackage => Paths::pack(&wardrobe.pack),
            _ => Paths::resource(&wardrobe.pack, options.subfolders),
        };
        let kind = output.kind.clone();
        (
            Self {
                paths,
                options,
                kind,
            },
            problems,
        )
    }

    /// Whether the files of the build are those of a singleplayer DLC pack.
    pub fn is_pack(&self) -> bool {
        self.paths.is_pack()
    }

    /// The pack as this build writes it: each collection spelled with the build's prefix, and a garment its tag rule leaves out kept as a place with no model.
    ///
    /// ⚠ A place and not a removal: a garment taken out would move the ones after it, and a frozen pack refuses that.
    pub fn pack<'a>(&self, wardrobe: &'a Wardrobe) -> Cow<'a, Wardrobe> {
        let tags = &self.options.include_tags;
        if self.options.prefix.is_empty() && tags.is_empty() {
            return Cow::Borrowed(wardrobe);
        }
        let mut named = wardrobe.clone();
        if !self.options.prefix.is_empty() {
            for ped in Ped::ALL {
                let collection = format!("{}{}", self.options.prefix, wardrobe.collection(ped));
                named.collections.insert(ped, collection);
            }
        }
        if !tags.is_empty() {
            for g in named
                .garments
                .iter_mut()
                .filter(|g| !g.carries_a_tag_of(tags))
            {
                g.hold_its_place();
            }
        }
        Cow::Owned(named)
    }
}

/// The sentence a finding that stops a build becomes in a plan's problems: what is wrong, with no rule code in front.
pub fn problem(finding: &Finding) -> String {
    finding.message.clone()
}

/// Whether the clothing `output` gets is what any FiveM resource gets, so a screen that planned a pack once may hand that plan to it.
pub fn is_standard(output: &Output) -> bool {
    let (options, problems) = Options::of(output);
    output.kind == OutputKind::FivemResource && options.is_default() && problems.is_empty()
}

/// What `wardrobe` hands to the plan of module `module`, whose folder is `folder`, given what the disk said of its files.
pub fn plan(wardrobe: &Wardrobe, facts: &Facts, module: &str, folder: &Path) -> Planned {
    plan_for(wardrobe, facts, module, folder, &Build::standard(wardrobe))
}

/// [`plan`] for the build `build`: its layout, and what its options leave out or add.
pub fn plan_for(
    wardrobe: &Wardrobe,
    facts: &Facts,
    module: &str,
    folder: &Path,
    build: &Build,
) -> Planned {
    let shaped = build.pack(wardrobe);
    let wardrobe = shaped.as_ref();
    let allocation = allocate(wardrobe);
    let findings = check(wardrobe, &allocation, facts);
    let mut contribution = Contribution::of(module);
    contribution.pack = build.is_pack();
    contribution.problems = findings
        .iter()
        .filter(|f| f.blocks())
        .map(problem)
        .collect();
    let mut documents = Documents::default();
    let mut memory = 0u64;
    let mut owners: Vec<(String, usize)> = Vec::new();

    for placed in allocation
        .placed
        .iter()
        .filter(|p| !wardrobe.garments[p.garment].is_reserved())
    {
        let first = contribution.files.len();
        copy_files(
            wardrobe,
            placed,
            (facts, folder),
            build,
            (&mut contribution, &mut memory),
        );
        owners
            .extend((contribution.files[first..].iter()).map(|f| (f.path.clone(), placed.garment)));
    }
    carry_shop_files(wardrobe, facts, folder, build, &mut contribution);
    crate::replace_mode::note(wardrobe, &allocation, build, &mut contribution);

    // ⚠ The files of a game collection are the game's own numbers, and the game's metadata states them: only the garments that add a collection of their own get metadata.
    let written = Allocation {
        placed: (allocation.placed.iter())
            .filter(|p| wardrobe.replaces(p.ped).is_none())
            .copied()
            .collect(),
        refused: Vec::new(),
    };
    let allocation_of_metadata = &written;
    let mut creatures: BTreeMap<u32, _> = BTreeMap::new();
    for part in written
        .placed
        .iter()
        .map(|p| p.part)
        .collect::<BTreeSet<_>>()
    {
        if let Some(creature) = metadata::creature_of(wardrobe, allocation_of_metadata, part) {
            creatures.insert(part, creature);
        }
    }
    if !build.options.creature_metadata && !creatures.is_empty() {
        creatures.clear();
        let who = (written.placed.iter())
            .filter(|p| !wardrobe.garments[p.garment].expressions.is_empty())
            .count();
        contribution.notes.push(format!(
            "this build leaves the creature metadata out, so the expressions of {who} garment(s), a heel among them, are not written"
        ));
    }
    documents.creature = creatures.remove(&0);
    documents.later_creatures = creatures;
    let mut drawables = 0u64;
    // The two kinds the total is made of: the drawables of the component slots, and those of the props.
    let (mut slot_drawables, mut prop_drawables) = (0u64, 0u64);
    let mut textures = 0u64;
    for ped in Ped::ALL {
        let mut parts = written.parts_of(ped);
        let kept = wardrobe.shop.unplaced.iter().any(|s| s.ped == ped);
        // ⚠ A ped with no garment still gets its shop file when an import kept entries for it: they are written back as stated, never dropped.
        if parts.is_empty() && kept && wardrobe.replaces(ped).is_none() {
            parts.push(0);
        }
        for part in parts {
            let garments = written
                .placed
                .iter()
                .any(|p| p.ped == ped && p.part == part);
            let collection = wardrobe.collection_of(ped, part);
            if garments {
                let info = metadata::variation_info_of(
                    wardrobe,
                    allocation_of_metadata,
                    (ped, part),
                    build.options.alternates,
                );
                let components = (info.a_component_data3.iter())
                    .map(|c| c.a_drawbl_data3.len() as u64)
                    .sum::<u64>();
                let props = info.prop_info.a_prop_meta_data.len() as u64;
                slot_drawables += components;
                prop_drawables += props;
                drawables += components + props;
                textures += info
                    .a_component_data3
                    .iter()
                    .flat_map(|c| &c.a_drawbl_data3)
                    .map(|d| d.a_tex_data.len() as u64)
                    .sum::<u64>()
                    + info
                        .prop_info
                        .a_prop_meta_data
                        .iter()
                        .map(|p| p.tex_data.len() as u64)
                        .sum::<u64>();
                match info.to_bytes() {
                    Ok(bytes) => contribution.files.push(PlannedFile {
                        path: build.paths.variation_path(ped, &collection),
                        origin: Origin::Generated(bytes),
                    }),
                    Err(e) => contribution.problems.push(format!(
                        "the variation metadata of {ped} cannot be written: {e}"
                    )),
                }
                documents.put_variation((ped, part), info);
            }
            contribution.claims.push(Claim {
                kind: COLLECTION.to_string(),
                name: collection.clone(),
            });
            let shop = build.paths.shop_path(ped, &collection);
            contribution.manifest.push(ManifestEntry {
                key: "data_file".to_string(),
                values: vec!["SHOP_PED_APPAREL_META_FILE".to_string(), shop.clone()],
            });
            let text = metadata::shop_meta_of(
                wardrobe,
                allocation_of_metadata,
                (ped, part),
                documents.creature_of(part).is_some(),
            );
            contribution.files.push(PlannedFile {
                path: shop,
                origin: Origin::Document(text.clone()),
            });
            documents.put_shop((ped, part), text);
        }
    }
    let creature_files: Vec<_> = (documents.creature.iter().map(|c| (0, c)))
        .chain(documents.later_creatures.iter().map(|(part, c)| (*part, c)))
        .map(|(part, c)| (part, c.to_bytes()))
        .collect();
    for (part, bytes) in creature_files {
        match bytes {
            Ok(bytes) => contribution.files.push(PlannedFile {
                path: build.paths.creature_path_of(part),
                origin: Origin::Generated(bytes),
            }),
            Err(e) => contribution
                .problems
                .push(format!("the creature metadata cannot be written: {e}")),
        }
    }
    alternate_file(
        wardrobe,
        allocation_of_metadata,
        build,
        &mut contribution,
        &mut documents,
    );
    first_person_file(
        wardrobe,
        allocation_of_metadata,
        build,
        &mut contribution,
        &mut documents,
    );
    dump_files(wardrobe, allocation_of_metadata, build, &mut contribution);
    crate::credits::contribute(wardrobe, folder, build, &mut contribution);

    contribution.figures = vec![
        Figure {
            name: "clothing_drawables".to_string(),
            amount: drawables,
        },
        Figure {
            name: "clothing_textures".to_string(),
            amount: textures,
        },
        Figure {
            name: "clothing_slots".to_string(),
            amount: slot_drawables,
        },
        Figure {
            name: "clothing_props".to_string(),
            amount: prop_drawables,
        },
        Figure {
            name: "clothing_outfit_memory".to_string(),
            amount: crate::outfit::heaviest_bytes(wardrobe, &allocation, facts),
        },
    ];
    contribution.figures.extend(meter(
        wardrobe,
        allocation_of_metadata,
        &contribution.files,
        memory,
    ));
    contribution.items = allocation
        .placed
        .iter()
        .map(|p| item(wardrobe, p, facts))
        .collect();
    let counts = crate::query::counts_of(wardrobe, facts, folder);
    let dependencies = crate::sizes::dependencies(wardrobe, folder, facts);
    Planned {
        contribution,
        allocation,
        findings,
        counts,
        owners,
        dependencies,
        documents,
        build: build.clone(),
    }
}

/// The alternate variations the pack's sets make, as a file the manifest registers; none when the build leaves alternates out.
fn alternate_file(
    wardrobe: &Wardrobe,
    allocation: &Allocation,
    build: &Build,
    contribution: &mut Contribution,
    documents: &mut Documents,
) {
    let Some(written) = crate::alternate::document(wardrobe, allocation) else {
        contribution
            .notes
            .extend(crate::alternate::notes(wardrobe, allocation));
        return;
    };
    if !build.options.alternates {
        contribution.notes.push(format!(
            "this build leaves the alternate models out, so the {} switch(es) of alternate variations are not written",
            written.switches
        ));
        return;
    }
    contribution.notes.extend(written.notes);
    match written.root.to_bytes() {
        Ok(bytes) => {
            let path = build.paths.alternate_path();
            contribution.manifest.push(ManifestEntry {
                key: "data_file".to_string(),
                values: vec![crate::alternate::FILE_TYPE.to_string(), path.clone()],
            });
            contribution.files.push(PlannedFile {
                path,
                origin: Origin::Generated(bytes),
            });
            documents.alternates = Some(written.root);
        }
        Err(e) => contribution
            .problems
            .push(format!("the alternate variations cannot be written: {e}")),
    }
}

/// The first person alternates the pack's garments state, as a file the manifest registers; none when the build leaves alternate models out.
fn first_person_file(
    wardrobe: &Wardrobe,
    allocation: &Allocation,
    build: &Build,
    contribution: &mut Contribution,
    documents: &mut Documents,
) {
    let Some(written) = crate::firstperson::document(wardrobe, allocation) else {
        return;
    };
    if !build.options.alternates {
        contribution.notes.push(format!(
            "this build leaves the alternate models out, so the {} first person alternate(s) are not written",
            written.count
        ));
        return;
    }
    match written.root.to_bytes() {
        Ok(bytes) => {
            let path = build.paths.first_person_path();
            contribution.manifest.push(ManifestEntry {
                key: "data_file".to_string(),
                values: vec![crate::firstperson::FILE_TYPE.to_string(), path.clone()],
            });
            contribution.files.push(PlannedFile {
                path,
                origin: Origin::Generated(bytes),
            });
            documents.first_person = Some(written.root);
        }
        Err(e) => contribution.problems.push(format!(
            "the first person alternates cannot be written: {e}"
        )),
    }
}

/// The clothes dump the build asks for, as files of a resource; a pack cannot carry one, and a note says it was left out.
///
/// ⚠ A file of a server's scripts is not a file the game is told about, so a singleplayer DLC has no type to register it with.
fn dump_files(
    wardrobe: &Wardrobe,
    allocation: &Allocation,
    build: &Build,
    contribution: &mut Contribution,
) {
    if build.options.dump == DumpKind::Off {
        return;
    }
    if build.is_pack() {
        contribution.notes.push(
            "the clothes dump is for the scripts of a server and a pack registers only files the game reads, so this build leaves it out; a FiveM resource carries it".to_string(),
        );
        return;
    }
    let dump = crate::dump::dump(wardrobe, allocation);
    contribution.files.push(PlannedFile {
        path: crate::dump::JSON.to_string(),
        origin: Origin::Document(dump.json()),
    });
    if build.options.dump == DumpKind::Both {
        contribution.files.push(PlannedFile {
            path: crate::dump::LUA.to_string(),
            origin: Origin::Document(dump.lua()),
        });
    }
}

/// The figures of the meter of the Wardrobe besides its drawables and textures, against the ceilings an output declares.
///
/// The memory is what the game streams for the files the plan copies, by the pages their headers state, and a file nobody counted adds none.
fn meter(
    wardrobe: &Wardrobe,
    allocation: &Allocation,
    files: &[PlannedFile],
    memory: u64,
) -> [Figure; 4] {
    let written: usize = files
        .iter()
        .map(|f| match &f.origin {
            Origin::Generated(bytes) => bytes.len(),
            Origin::Document(text) => text.len(),
            _ => 0,
        })
        .sum();
    // An archive holds the folders its files are in as entries too.
    let mut entries: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for file in files {
        let mut path = file.path.as_str();
        entries.insert(path);
        while let Some((parent, _)) = path.rsplit_once('/') {
            entries.insert(parent);
            path = parent;
        }
    }
    let figure = |name: &str, amount: usize| Figure {
        name: name.to_string(),
        amount: amount as u64,
    };
    [
        figure(
            "clothing_shop_entries",
            shop::written(wardrobe, allocation).len(),
        ),
        figure("clothing_metadata_bytes", written),
        figure("clothing_archive_entries", entries.len()),
        Figure {
            name: "clothing_memory".to_string(),
            amount: memory,
        },
    ]
}

/// The files a manifest registered with the shop that this engine does not read, shipped as they are under the lines that registered them.
fn carry_shop_files(
    wardrobe: &Wardrobe,
    facts: &Facts,
    folder: &Path,
    build: &Build,
    contribution: &mut Contribution,
) {
    let mut taken: Vec<String> = Vec::new();
    for carried in &wardrobe.shop.carried {
        let to = build.paths.carried_path(carried);
        if taken.iter().any(|t| t.eq_ignore_ascii_case(&to)) {
            contribution.problems.push(format!(
                "two files registered with the shop would ship as {to}, and the second is {}",
                carried.file
            ));
            continue;
        }
        let Some(Fact::File { bytes, .. }) = facts.get(&carried.file) else {
            contribution.problems.push(format!(
                "{}, registered with the shop as {}, is not a file in the pack's folder",
                carried.file, carried.kind
            ));
            continue;
        };
        contribution.files.push(PlannedFile {
            path: to.clone(),
            origin: Origin::Copy {
                from: folder.join(carried.file.replace('/', std::path::MAIN_SEPARATOR_STR)),
                bytes: *bytes,
            },
        });
        contribution.manifest.push(ManifestEntry {
            key: carried.directive.clone(),
            values: vec![carried.kind.clone(), to.clone()],
        });
        taken.push(to);
    }
}

/// The copy of the model and the textures of a garment to the files the layout names.
///
/// A name that is a source folder ships the file the build cooks it into, under the name any other file of its place has,
/// whichever platform the build states; one that is a reference ships what it leads to, and one that is only an id ships
/// nothing and is said in a note.
fn copy_files(
    wardrobe: &Wardrobe,
    placed: &Placed,
    (facts, folder): (&Facts, &Path),
    build: &Build,
    (contribution, memory): (&mut Contribution, &mut u64),
) {
    let garment = &wardrobe.garments[placed.garment];
    let who = crate::rules::name_of(garment);
    let collection = wardrobe.collection_of(placed.ped, placed.part);
    let dir = build.paths.directory(placed.ped, &collection, placed.slot);
    let mut copy = |source: &str, name: String, part: &str, kind: &str| {
        let (from, folder_source) = match crate::follow::follow(folder, source) {
            Led::Id(id) => {
                contribution
                    .notes
                    .push(crate::follow::named_only(&who, part, &id));
                return;
            }
            Led::Nowhere { .. } => return,
            Led::To { at, source } => (at, source),
            Led::Own => (
                folder.join(source.replace('/', std::path::MAIN_SEPARATOR_STR)),
                crate::facts::is_source(source),
            ),
        };
        if let Some(Fact::File { bytes, .. }) = facts.get(source) {
            *memory +=
                crate::measure::Measure::of_fact(facts.get(source)).map_or(0, |m| m.streaming);
            // ⚠ Never as a file of the game's own: the pack's metadata states this drawable, so its file ships whatever it equals.
            let origin = match folder_source {
                true => Origin::Cook {
                    source: from,
                    kind: kind.to_string(),
                    name: name
                        .rsplit_once('.')
                        .map_or(name.as_str(), |(s, _)| s)
                        .to_string(),
                    bytes: *bytes,
                    game: Vec::new(),
                },
                false => Origin::Copy {
                    from,
                    bytes: *bytes,
                },
            };
            contribution.files.push(PlannedFile {
                path: format!("{dir}/{name}"),
                origin,
            });
        }
    };
    let drawables = or_source::models::DRAWABLES;
    let textures = or_source::textures::KIND;
    if let Some(model) = &garment.model {
        let name = build.paths.model_name(
            placed.ped,
            &collection,
            placed.slot,
            placed.position,
            garment.skin,
        );
        let alternates = garment
            .alternates
            .iter()
            .take(match build.options.alternates {
                true => usize::MAX,
                false => 0,
            });
        for (i, alternate) in alternates.enumerate() {
            copy(
                alternate,
                layout::alternate_name(&name, i + 1),
                "model",
                drawables,
            );
        }
        // ⚠ Only a component owns cloth (`ownsCloth` is a field of a drawable's data and a prop has none), and the game finds it by the model's name.
        if let (Some(cloth), false) = (&garment.cloth, placed.slot.is_prop()) {
            copy(cloth, layout::cloth_name(&name), "cloth", "");
        }
        copy(model, name, "model", drawables);
    }
    for (i, variation) in garment.variations.iter().enumerate() {
        let Some(texture) = &variation.texture else {
            continue;
        };
        let tint = variation.tint.as_deref().unwrap_or(garment.skin.tint());
        copy(
            texture,
            build.paths.texture_name(
                placed.ped,
                &collection,
                placed.slot,
                placed.position,
                i,
                tint,
            ),
            "texture",
            textures,
        );
    }
    if let (Some(scale), Some(_), Some(_), false) = (
        garment.wind_scale,
        &garment.cloth,
        &garment.model,
        placed.slot.is_prop(),
    ) {
        let name = build.paths.model_name(
            placed.ped,
            &collection,
            placed.slot,
            placed.position,
            garment.skin,
        );
        let path = format!("{dir}/{}", layout::cloth_name(&name));
        state_wind(contribution, &path, scale, &who);
    }
}

/// Ship the cloth the plan copies to `path` as the same file stating the wind scale `scale` in each of its cloths.
fn state_wind(contribution: &mut Contribution, path: &str, scale: f32, who: &str) {
    let Some(file) = contribution.files.iter_mut().find(|f| f.path == path) else {
        return;
    };
    let Origin::Copy { from, .. } = &file.origin else {
        return;
    };
    let stated = std::fs::read(from)
        .map_err(|e| e.to_string())
        .and_then(|bytes| crate::wind::with_scale(&bytes, scale));
    match stated {
        Ok(bytes) => file.origin = Origin::Generated(bytes),
        Err(why) => contribution.problems.push(format!(
            "the cloth of {who} cannot be given the wind scale {scale}: {why}"
        )),
    }
}

/// The name the game sees a garment by, which is what a saved outfit points at: its collection, its slot and its position.
pub fn place_name(wardrobe: &Wardrobe, placed: &Placed) -> String {
    let collection = wardrobe.collection_of(placed.ped, placed.part);
    format!(
        "{}^{}_{:03}",
        layout::stem(placed.ped, &collection, placed.slot.is_prop()),
        placed.slot.name(),
        placed.position
    )
}

/// The item of a placed garment: its id, the name the game sees and a digest of everything its files hold.
fn item(wardrobe: &Wardrobe, placed: &Placed, facts: &Facts) -> Item {
    let garment = &wardrobe.garments[placed.garment];
    Item {
        id: garment.id.clone(),
        name: place_name(wardrobe, placed),
        digest: digest(garment, facts),
    }
}

/// What a garment writes, apart from where: its skin, textures, metadata and expressions, and the bytes of its files.
fn digest(garment: &Garment, facts: &Facts) -> String {
    let content = |path: &Option<String>| match path.as_deref().and_then(|p| facts.get(p)) {
        Some(Fact::File {
            digest: Some(d), ..
        }) => d.clone(),
        _ => String::new(),
    };
    let variations: Vec<_> = garment
        .variations
        .iter()
        .map(|v| {
            let mut stripped = v.clone();
            stripped.texture = None;
            json!({ "variation": stripped, "content": content(&v.texture) })
        })
        .collect();
    let alternates: Vec<String> = garment
        .alternates
        .iter()
        .map(|a| content(&Some(a.clone())))
        .collect();
    let mut state = json!({
        "skin": garment.skin,
        "model": content(&garment.model),
        "alternates": alternates,
        "variations": variations,
        "meta": garment.meta,
        "expressions": garment.expressions,
    });
    // ⚠ Left out when there is none, so a garment with no cloth keeps the digest the builds before cloth travelled recorded for it.
    if garment.cloth.is_some() {
        state["cloth"] = json!(content(&garment.cloth));
    }
    if garment.missing_cloth {
        state["missing_cloth"] = json!(true);
    }
    // ⚠ Left out when there is none, as the cloth is: a garment that states no wind keeps the digest it had.
    if let Some(scale) = garment.wind_scale.filter(|_| garment.cloth.is_some()) {
        state["wind_scale"] = json!(scale);
    }
    let text = serde_json::to_string(&state).expect("a JSON value serialises");
    crate::facts::digest(Sha256::digest(text.as_bytes()).as_slice())
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Wardrobe(#[from] model::Error),
    #[error("the folder of the content {module} is {why}")]
    Folder { module: String, why: String },
    #[error("{0}")]
    Linked(String),
}

/// The folder a clothing module's content is in: inside `project_folder`, or where its link leads.
pub fn module_folder(project_folder: &Path, module: &Module) -> Result<PathBuf, Error> {
    use or_project::content::folder::{module_folder, FolderError};
    module_folder(project_folder, module, "a clothing module").map_err(|e| match e {
        FolderError::Folder { module, why } => Error::Folder { module, why },
        FolderError::Linked(why) => Error::Linked(why),
    })
}

/// The folder a clothing module's pack is edited in: [`module_folder`], except that a link is refused.
///
/// ⚠ The project that holds a linked folder is the only one that writes it, so anything that edits a pack asks here for the folder to write.
pub fn module_folder_to_edit(
    project: &Path,
    container: &Container,
    module: &Module,
) -> Result<PathBuf, Error> {
    let folder = module_folder(&or_project::project_folder(project), module)?;
    or_project::links::refuse_write(container, project, &folder.join(model::FILE))
        .map_err(Error::Linked)?;
    Ok(folder)
}

/// A pack read from the folder of module `module` of the project in `project_folder`, and planned.
pub fn planned(project_folder: &Path, module: &Module, depth: Depth) -> Result<Planned, Error> {
    planned_with_model(project_folder, module, depth).map(|(_, planned)| planned)
}

/// The same, with the pack that was planned: a screen draws the garments and the plan's findings of one reading.
pub fn planned_with_model(
    project_folder: &Path,
    module: &Module,
    depth: Depth,
) -> Result<(Wardrobe, Planned), Error> {
    let folder = module_folder(project_folder, module)?;
    let wardrobe = Wardrobe::read(&folder)?;
    crate::cache::load(&folder);
    let facts = inspect(&folder, &wardrobe, depth);
    let planned = plan(&wardrobe, &facts, &module.id, &folder);
    Ok((wardrobe, planned))
}

/// A pack read from the folder of module `module` and planned for the build `output`, with the pack as that build writes it.
///
/// ⚠ The pack returned is the one the build writes (its collections spelled with the build's prefix), which a verifier is held to, and its options that cannot be used are in the contribution's problems.
pub fn planned_for(
    project_folder: &Path,
    module: &Module,
    depth: Depth,
    output: &Output,
) -> Result<(Wardrobe, Planned), Error> {
    let folder = module_folder(project_folder, module)?;
    let wardrobe = Wardrobe::read(&folder)?;
    crate::cache::load(&folder);
    let facts = inspect(&folder, &wardrobe, depth);
    let (build, problems) = Build::of(&wardrobe, output);
    let mut planned = plan_for(&wardrobe, &facts, &module.id, &folder, &build);
    planned.contribution.problems.extend(problems);
    Ok((build.pack(&wardrobe).into_owned(), planned))
}

/// What module `module` hands to the build `output`: the files as that kind of build lays them out, with its options applied.
pub fn contribution_for(project_folder: &Path, module: &Module, output: &Output) -> Contribution {
    match planned_for(project_folder, module, Depth::Measure, output) {
        Ok((_, planned)) => planned.contribution,
        Err(e) => {
            let mut contribution = Contribution::of(&module.id);
            contribution.problems.push(e.to_string());
            contribution
        }
    }
}

/// What module `module` hands over; a module whose folder or `wardrobe.json` cannot be read hands over the reason.
pub fn contribution(project_folder: &Path, module: &Module) -> Contribution {
    match planned(project_folder, module, Depth::Measure) {
        Ok(planned) => planned.contribution,
        Err(e) => {
            let mut contribution = Contribution::of(&module.id);
            contribution.problems.push(e.to_string());
            contribution
        }
    }
}

/// What every clothing module of `container` hands over: all of them, because the names the game resolves once are one namespace for the whole project.
pub fn contributions(container: &Container, project_folder: &Path) -> Vec<Contribution> {
    container
        .modules
        .iter()
        .filter(|m| m.kind == ModuleKind::Clothing)
        .map(|m| contribution(project_folder, m))
        .collect()
}

/// The plans of `output` of `project`, the Files modules' contributions beside the clothing modules'.
pub fn plans(
    project: &or_project::Project,
    output: &str,
) -> Result<Vec<or_project::Plan>, or_project::NoSuchOutput> {
    plans_from(
        &project.container,
        output,
        &project.folder(),
        &project.sources(),
    )
}

/// What every clothing module of `container` hands to the build `output`, laid out as that kind of build lays files out and with its options.
pub fn contributions_to(
    container: &Container,
    project_folder: &Path,
    output: &Output,
) -> Vec<Contribution> {
    container
        .modules
        .iter()
        .filter(|m| m.kind == ModuleKind::Clothing)
        .map(|m| contribution_for(project_folder, m, output))
        .collect()
}

/// The same for a project given as its parts: its container, its folder and the files it holds as the disk shows them.
pub fn plans_from(
    container: &Container,
    output: &str,
    folder: &Path,
    held: &[or_project::Source],
) -> Result<Vec<or_project::Plan>, or_project::NoSuchOutput> {
    let declared = container
        .output(output)
        .ok_or_else(|| or_project::NoSuchOutput(output.to_string()))?;
    plans_given(
        container,
        output,
        folder,
        held,
        contributions_to(container, folder, declared),
    )
}

/// The same with the clothing contributions already made, as a screen that has planned each pack hands them over.
pub fn plans_given(
    container: &Container,
    output: &str,
    folder: &Path,
    held: &[or_project::Source],
    clothing: Vec<Contribution>,
) -> Result<Vec<or_project::Plan>, or_project::NoSuchOutput> {
    let mut handed = or_project::files_contributions(container, output, folder, held)?;
    handed.extend(clothing);
    or_project::plans_with(container, output, folder, &handed)
}
