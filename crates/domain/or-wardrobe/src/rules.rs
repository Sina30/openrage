//! The rules: what is wrong with a pack, as data with a code, a severity and a waiver.
//!
//! A rule is a row of [`RULES`], a finding is one place a rule applies, and a waiver is a line of
//! the pack that says why a finding stands. Only an error that no reason could mend, such as a
//! file that is not there, refuses to be waived.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

mod export;
mod measured;

use crate::alloc::{Allocation, Refused, MAX_PARTS, MAX_POSITION};
use crate::facts::{Fact, Facts};
use crate::model::{Garment, Wardrobe};
use crate::names::{is_identifier, Slot};

pub use export::{export, export_text, read_export, Exported, Limit};
pub use or_project::content::rule::{Rule, Severity};

const fn rule(
    code: &'static str,
    severity: Severity,
    waivable: bool,
    summary: &'static str,
) -> Rule {
    Rule::new(code, severity, waivable, summary)
}

use Severity::{Error, Info, Warning};

/// Every rule, in the order a report lists them.
pub const RULES: &[Rule] = &[
    rule(
        "pack-name",
        Error,
        false,
        "the pack or a collection is not a lower case name the game resolves",
    ),
    rule(
        "garment-unplaced",
        Error,
        false,
        "a garment has no ped or no slot",
    ),
    rule(
        "position-clash",
        Error,
        false,
        "two garments are pinned at one place of a slot",
    ),
    rule(
        "slot-cap",
        Warning,
        true,
        "a slot of a ped holds more drawables than the cap of one collection, so a build writes it as several collections; a cap of 255 is the most a collection holds",
    )
    .with_threshold(128, "drawables"),
    rule(
        "slot-full",
        Error,
        false,
        "a slot of a ped holds no more drawables than the collections a pack may split it into take",
    ),
    rule(
        "renumbered",
        Error,
        true,
        "a garment that the last build shipped is at another drawable now, so a saved outfit, a shop table or a script that names the old one would show another garment; a pack that freezes its drawables stops the build until the garment is pinned back or a reason says why",
    ),
    rule(
        "replace-mode",
        Info,
        true,
        "the files of a ped replace the game's own, and nobody has seen the game load a pack that does",
    ),
    rule(
        "replace-unpinned",
        Warning,
        true,
        "a garment whose files replace the game's own is pinned at no number, so it replaces the lowest free drawable and not the one meant",
    ),
    rule(
        "path-escapes",
        Error,
        false,
        "a file is named by a path that leaves the pack's folder",
    ),
    rule(
        "model-missing",
        Error,
        false,
        "the model a garment names is not there",
    ),
    rule(
        "texture-missing",
        Error,
        false,
        "a texture a garment names is not there",
    ),
    rule(
        "model-unreadable",
        Error,
        true,
        "the reader for the model refuses it",
    ),
    rule(
        "texture-unreadable",
        Error,
        true,
        "the reader for a texture refuses it",
    ),
    rule(
        "variation-none",
        Error,
        false,
        "a garment with a model has no texture, and a drawable wears one",
    ),
    rule(
        "variation-cap",
        Error,
        false,
        "a garment has more than 26 textures, which the letters of their names run out at",
    ),
    rule(
        "meta-out-of-range",
        Error,
        false,
        "a value of a garment's metadata does not fit the field it is for",
    ),
    rule(
        "alternate-reference",
        Error,
        false,
        "a set of alternate variations names a garment the pack does not place for its ped",
    ),
    rule(
        "alternate-alt",
        Error,
        false,
        "a switch of alternate variations wears alternate 0, or an alternate a drawable of the pack has no model for",
    ),
    rule(
        "alternate-limit",
        Error,
        false,
        "one garment makes more drawables change than the game holds for it",
    ),
    rule(
        "first-person-alternate",
        Error,
        false,
        "a first person alternate is stated for a prop, or is a number the garment has no alternate model for",
    ),
    rule(
        "mask-skin",
        Error,
        false,
        "a drawable's propMask asks the game for another form of its files (u, r or m) than the one the pack ships",
    ),
    rule(
        "expression-arrays",
        Error,
        false,
        "the arrays of an expression are not one length",
    ),
    rule(
        "creature-clash",
        Error,
        false,
        "two garments of one place state different expressions in the one creature metadata",
    ),
    rule(
        "waiver-without-reason",
        Error,
        false,
        "a waiver gives no reason",
    ),
    rule("waiver-unknown", Warning, false, "a waiver names no rule"),
    rule(
        "duplicate-content",
        Warning,
        true,
        "two garments of one type ship the same model and the same textures",
    ),
    rule(
        "texture-count-wraps",
        Warning,
        true,
        "a slot holds 256 textures or more, and the byte that states them wraps",
    ),
    rule(
        "cloth-missing",
        Error,
        true,
        "a garment is missing its cloth: its metadata states ownsCloth and no cloth file is linked",
    ),
    rule(
        "cloth-file-missing",
        Error,
        false,
        "the cloth a garment links is not a file",
    ),
    rule(
        "cloth-ignored",
        Warning,
        true,
        "a cloth is linked to a garment the game reads none for: a prop owns no cloth, and a place held has no model to name it after",
    ),
    rule(
        "cloth-unreadable",
        Error,
        true,
        "the reader for the cloth refuses it",
    ),
    rule(
        "shop-id-clash",
        Warning,
        true,
        "two shop entries are written with one id",
    ),
    rule(
        "shop-entry-unplaced",
        Warning,
        true,
        "shop entries an import read point at no garment, and are written back as they were stated",
    ),
    rule(
        "texture-size",
        Warning,
        true,
        "a texture is wider or taller than the limit, which everyone who sees the garment pays for in memory and load time",
    )
    .with_threshold(2048, "pixels"),
    rule(
        "texture-skin-tone-size",
        Warning,
        true,
        "a skin-tone texture of a garment that fits the race skins is not square at the size the skin tones are held to, 512 pixels a side unless the project says otherwise",
    )
    .with_threshold(512, "pixels"),
    rule(
        "texture-not-power-of-two",
        Warning,
        true,
        "a texture's width or height is not a power of two",
    ),
    rule(
        "texture-mips",
        Warning,
        true,
        "a texture larger than 4 by 4 pixels has no smaller sizes (mipmaps), so it shimmers where the garment is far away",
    ),
    rule(
        "texture-format",
        Warning,
        true,
        "a texture is stored in a format that the game's own clothing is not, and one that is not compressed takes four times the memory",
    ),
    rule(
        "texture-bc7-legacy",
        Warning,
        true,
        "a texture is BC7, which a DirectX 10 graphics card cannot read, in a pack that is built for Legacy",
    ),
    rule(
        "dictionary-size",
        Warning,
        true,
        "a texture dictionary takes more memory than the limit, and the game streams a dictionary in whole",
    )
    .with_threshold(16 * 1024 * 1024, "bytes"),
    rule(
        "lod-missing",
        Info,
        true,
        "a model has a high level of detail and no medium or no low one, so it is drawn at its full cost wherever it is seen from; the game's own models often have none, so this is a note until a project makes it a warning",
    ),
    rule(
        "lod-medium-heavy",
        Warning,
        true,
        "the medium level of detail of a model has more than the limit's share of the vertices of its high one, so it saves too little",
    )
    .with_threshold(50, "percent"),
    rule(
        "lod-low-heavy",
        Warning,
        true,
        "the low level of detail of a model has more vertices than the limit",
    )
    .with_threshold(500, "vertices"),
    rule(
        "polycount-high",
        Warning,
        true,
        "the high level of detail of a model draws more triangles than the limit",
    )
    .with_threshold(35_000, "triangles"),
    rule(
        "diffuse-missing",
        Warning,
        true,
        "a shader of a model binds no diffuse texture, so the garment shows none",
    ),
    rule(
        "embedded-unused",
        Warning,
        true,
        "a model carries a texture that none of its shaders uses, which adds its size to every streaming of the model",
    ),
    rule(
        "material-parameters",
        Warning,
        true,
        "a material of a model lacks constants its shader declares, so the game uses the shader's own values for them; completing the material states those values",
    ),
    rule(
        "setting-unheeded",
        Warning,
        false,
        "the rules of the pack set a rule that is not there, one that no reason mends, or a limit that a rule has none of",
    ),
    rule(
        "reserved",
        Info,
        true,
        "a garment holds a place and ships no model",
    ),
];

pub fn rule_of(code: &str) -> Option<&'static Rule> {
    RULES.iter().find(|r| r.code == code)
}

/// One place a rule applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub code: &'static str,
    pub severity: Severity,
    /// The id of the garment, none for the pack.
    pub garment: Option<String>,
    pub message: String,
    /// The reason of the waiver that sets it aside.
    pub waived: Option<String>,
    /// For a finding about a waiver, which one: its index in the pack's list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiver: Option<usize>,
    /// For a finding about one texture of a garment, which one: its index among the garment's textures.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture: Option<usize>,
}

impl Finding {
    /// Whether a build must stop for it.
    pub fn blocks(&self) -> bool {
        self.severity == Severity::Error && self.waived.is_none()
    }
}

/// What to call a garment in a sentence: its name as a person reads it, with no mark around it.
pub fn name_of(garment: &Garment) -> String {
    if let Some(label) = &garment.label {
        return label.clone();
    }
    match garment.model.as_deref().and_then(|m| m.rsplit('/').next()) {
        Some(file) => file.to_string(),
        None => format!(
            "the garment {}",
            garment.id.chars().take(8).collect::<String>()
        ),
    }
}

struct Report<'a> {
    wardrobe: &'a Wardrobe,
    found: Vec<Finding>,
}

impl Report<'_> {
    fn add(&mut self, code: &'static str, garment: Option<&Garment>, message: String) {
        self.add_about(code, garment, None, message);
    }

    /// A finding about texture `texture` of its garment, at the severity the pack sets the rule to; none where the pack switched the rule off.
    fn add_about(
        &mut self,
        code: &'static str,
        garment: Option<&Garment>,
        texture: Option<usize>,
        message: String,
    ) {
        let severity = match rule_of(code) {
            Some(rule) => {
                let set = self.wardrobe.rules.effective(rule);
                if !set.on {
                    return;
                }
                set.severity
            }
            None => Error,
        };
        self.found.push(Finding {
            code,
            severity,
            garment: garment.map(|g| g.id.clone()),
            message,
            waived: None,
            waiver: None,
            texture,
        });
    }

    /// Where the limit of the rule `code` stands for the pack.
    fn limit(&self, code: &str) -> u64 {
        let rule = rule_of(code).expect("a rule with a limit is a row of RULES");
        (self.wardrobe.rules.effective(rule).threshold)
            .expect("a rule with a limit has a threshold")
    }

    /// Say the finding just added is about texture `index` of its garment.
    fn about_texture(&mut self, index: usize) {
        if let Some(last) = self.found.last_mut() {
            last.texture = Some(index);
        }
    }
}

/// Every finding of `wardrobe`, with the waivers applied.
pub fn check(wardrobe: &Wardrobe, allocation: &Allocation, facts: &Facts) -> Vec<Finding> {
    let mut report = Report {
        wardrobe,
        found: Vec::new(),
    };
    pack_names(&mut report);
    for (index, garment) in wardrobe.garments.iter().enumerate() {
        one_garment(&mut report, index, garment, allocation, facts);
    }
    duplicates(&mut report, facts);
    slot_caps(&mut report, allocation);
    replaced(&mut report, allocation);
    renumbered(&mut report, allocation);
    wraps(&mut report, allocation);
    creature(&mut report, allocation);
    alternates(&mut report, allocation);
    first_person(&mut report);
    shop(&mut report, allocation);
    unheeded(&mut report);
    waivers(&mut report);
    report.found
}

fn pack_names(r: &mut Report) {
    let w = r.wardrobe;
    if !is_identifier(&w.pack) {
        r.add("pack-name", None, format!("the pack is named {:?}", w.pack));
    }
    for (ped, replace) in &w.rules.replace {
        if let Some(dlc) = replace.dlc.as_deref().filter(|d| !is_identifier(d)) {
            r.add(
                "pack-name",
                None,
                format!("the collection of the game that {ped} replaces is named {dlc:?}"),
            );
        }
    }
    for (ped, collection) in &w.collections {
        if !is_identifier(collection) {
            r.add(
                "pack-name",
                None,
                format!("the collection of {ped} is named {collection:?}"),
            );
        }
    }
}

fn one_garment(r: &mut Report, index: usize, g: &Garment, allocation: &Allocation, facts: &Facts) {
    let who = name_of(g);
    if g.ped.is_none() || g.slot.is_none() {
        let lacks = match (g.ped, g.slot) {
            (None, None) => "ped and no slot",
            (None, _) => "ped",
            _ => "slot",
        };
        r.add("garment-unplaced", Some(g), format!("{who} has no {lacks}"));
    }
    match allocation.refused.iter().find(|(i, _)| *i == index) {
        Some((_, Refused::Taken { by })) => {
            let other = name_of(&r.wardrobe.garments[*by]);
            r.add(
                "position-clash",
                Some(g),
                format!(
                    "{who} is pinned at position {} of {}, where {other} is",
                    g.position.unwrap_or_default(),
                    g.slot.map_or(String::new(), |s| s.to_string())
                ),
            );
        }
        Some((_, Refused::PastTheEnd)) => {
            let held = match g.ped.filter(|p| r.wardrobe.replaces(*p).is_some()) {
                Some(_) => format!("{} drawables, the game's own numbers", MAX_POSITION + 1),
                None => format!(
                    "{} drawables, {} in each of {MAX_PARTS} collections",
                    r.wardrobe.rules.slot_cap() * MAX_PARTS,
                    r.wardrobe.rules.slot_cap()
                ),
            };
            r.add(
                "slot-full",
                Some(g),
                format!(
                    "{who} has no place left in {}, which holds {held}",
                    g.slot.map_or(String::new(), |s| s.to_string())
                ),
            )
        }
        None => {}
    }

    match &g.model {
        None => r.add(
            "reserved",
            Some(g),
            format!("{who} holds a place and ships no model"),
        ),
        Some(model) => {
            file(r, g, &who, "model", model, facts);
            for alternate in &g.alternates {
                file(r, g, &who, "model", alternate, facts);
            }
            if g.variations.is_empty() {
                r.add("variation-none", Some(g), format!("{who} has no texture"));
            }
        }
    }
    if g.variations.len() > 26 {
        r.add(
            "variation-cap",
            Some(g),
            format!(
                "{who} has {} textures, and their names run from a to z",
                g.variations.len()
            ),
        );
    }
    for (i, texture) in
        (g.variations.iter().enumerate()).filter_map(|(i, v)| Some((i, v.texture.as_deref()?)))
    {
        let before = r.found.len();
        file(r, g, &who, "texture", texture, facts);
        if r.found.len() > before {
            r.about_texture(i);
        }
    }
    if let Some(cloth) = &g.cloth {
        file(r, g, &who, "cloth", cloth, facts);
    }
    measured::garment(r, g, &who, facts);
    cloth(r, g, &who);
    if g.slot.is_some_and(Slot::is_prop) && g.meta.flags.is_some_and(|f| f > u32::from(u16::MAX)) {
        r.add(
            "meta-out-of-range",
            Some(g),
            format!(
                "{who} is a prop with flags of {}, and a prop's hold 16 bits",
                g.meta.flags.unwrap_or_default()
            ),
        );
    }
    mask_skin(r, g, &who);
    for mods in [g.meta.ped_xml_expression_mods, g.meta.expression_mods]
        .into_iter()
        .flatten()
    {
        if mods.iter().any(|m| !m.is_finite()) {
            r.add(
                "meta-out-of-range",
                Some(g),
                format!("{who} states an expression modifier that is no number"),
            );
        }
    }
    for expression in &g.expressions {
        let n = expression.tracks.len();
        if [
            expression.ids.len(),
            expression.types.len(),
            expression.components.len(),
        ]
        .iter()
        .any(|len| *len != n)
        {
            r.add(
                "expression-arrays",
                Some(g),
                format!("{who} has an expression, number {}, whose tracks, ids, types and components are not one length", expression.index),
            );
        }
    }
}

/// Whether the form of its files a drawable's `propMask` asks the game for is the one the pack ships: `m` with the match bit, `r` with the race bit, `u` otherwise.
fn mask_skin(r: &mut Report, g: &Garment, who: &str) {
    if g.slot.is_none_or(Slot::is_prop) || g.is_reserved() {
        return;
    }
    let asked = crate::mask::form_asked(g);
    if asked != g.skin.letter() {
        r.add(
            "mask-skin",
            Some(g),
            format!(
                "{who} is of the {} skin and its propMask asks the game for the {asked} form of its files, which the pack does not ship",
                g.skin.letter()
            ),
        );
    }
}

/// Whether a garment is missing its cloth, and whether a cloth linked to it is one the game reads.
///
/// ⚠ The plan ships a cloth beside a component's model and nowhere else: a prop has no `ownsCloth`,
/// and a place held has no model to be named after.
fn cloth(r: &mut Report, g: &Garment, who: &str) {
    if g.missing_cloth && g.cloth.is_none() {
        r.add(
            "cloth-missing",
            Some(g),
            format!("{who} is missing its cloth: its metadata states ownsCloth and no cloth file is linked"),
        );
    }
    if g.cloth.is_some() {
        let why = match (g.slot.is_some_and(Slot::is_prop), g.model.is_some()) {
            (true, _) => Some("a prop owns no cloth"),
            (_, false) => Some("it ships no model for the cloth to be named after"),
            _ => None,
        };
        if let Some(why) = why {
            r.add(
                "cloth-ignored",
                Some(g),
                format!("{who} has a cloth the game ignores: {why}"),
            );
        }
    }
}

/// Ids that two entries of the shop metadata claim, and entries an import kept for want of a garment.
fn shop(r: &mut Report, allocation: &Allocation) {
    let w = r.wardrobe;
    for (id, owners) in crate::shop::clashes(w, allocation) {
        // The finding is the last garment's that claims it, so a fix or a waiver names one.
        let garment = owners
            .iter()
            .rev()
            .find_map(|owner| w.garments.iter().find(|g| owner.starts_with(&name_of(g))));
        r.add(
            "shop-id-clash",
            garment,
            format!("the shop id {id} is written for {}", owners.join(" and ")),
        );
    }
    let mut by_file: BTreeMap<&str, Vec<&crate::model::Stated>> = BTreeMap::new();
    for stated in &w.shop.unplaced {
        by_file.entry(&stated.entry.from).or_default().push(stated);
    }
    for (from, list) in by_file {
        let first = list[0];
        r.add(
            "shop-entry-unplaced",
            None,
            format!(
                "{} entr{} of {from} {} no garment of the pack and are written back as stated; the first is {} {} {}, texture {}, id {}",
                list.len(),
                if list.len() == 1 { "y" } else { "ies" },
                if list.len() == 1 { "points at" } else { "point at" },
                first.ped,
                first.kind,
                first.drawable,
                first.texture,
                first.entry.id
            ),
        );
    }
}

fn file(r: &mut Report, g: &Garment, who: &str, what: &'static str, path: &str, facts: &Facts) {
    let (missing, unreadable): (&'static str, &'static str) = match what {
        "model" => ("model-missing", "model-unreadable"),
        "cloth" => ("cloth-file-missing", "cloth-unreadable"),
        _ => ("texture-missing", "texture-unreadable"),
    };
    // ⚠ A garment named by its model's file would say that name twice, so the path is left out where it adds nothing.
    let at = match path.rsplit('/').next() == Some(who) {
        true => String::new(),
        false => format!(" ({path})"),
    };
    match facts.get(path) {
        Some(Fact::Escapes) => r.add(
            "path-escapes",
            Some(g),
            format!("{who} names a {what} outside the pack's folder{at}"),
        ),
        Some(Fact::Missing) if or_project::reference::is_reference(path) => r.add(
            missing,
            Some(g),
            format!("{who} names a {what} through a reference that leads nowhere{at}"),
        ),
        Some(Fact::Missing) => r.add(
            missing,
            Some(g),
            format!("{who} names a {what} that is not there{at}"),
        ),
        Some(Fact::Folder) => r.add(
            missing,
            Some(g),
            format!("{who} names a {what} that is a folder, not a file{at}"),
        ),
        Some(Fact::File {
            read: Some(Err(why)),
            ..
        }) => r.add(
            unreadable,
            Some(g),
            format!("{who} names a {what} that does not open{at}: {why}"),
        ),
        _ => {}
    }
}

/// The bytes of the file `path`, as its digest, when the pack's files were digested and this one was read.
fn digest_of<'a>(facts: &'a Facts, path: &str) -> Option<&'a str> {
    match facts.get(path)? {
        Fact::File {
            digest: Some(digest),
            ..
        } => Some(digest),
        _ => None,
    }
}

/// What two garments must share to be one: their type, their model and every texture, by what the files hold.
type Content<'a> = (
    Option<crate::names::Ped>,
    Option<Slot>,
    &'a str,
    Vec<&'a str>,
);

/// Each garment of `wardrobe` that ships what an earlier one ships, as its index and the earlier one's; equal models with other textures are other garments.
pub(crate) fn duplicate_pairs(wardrobe: &Wardrobe, facts: &Facts) -> Vec<(usize, usize)> {
    let mut first: BTreeMap<Content<'_>, usize> = BTreeMap::new();
    let mut found = Vec::new();
    for (at, g) in wardrobe.garments.iter().enumerate() {
        let Some(model) = g.model.as_deref().and_then(|m| digest_of(facts, m)) else {
            continue;
        };
        // A texture that could not be read says nothing of what the garment ships, so the garment is not judged.
        let textures: Option<Vec<&str>> = (g.variations.iter())
            .filter_map(|v| v.texture.as_deref())
            .map(|t| digest_of(facts, t))
            .collect();
        let Some(mut textures) = textures else {
            continue;
        };
        textures.sort_unstable();
        match first.get(&(g.ped, g.slot, model, textures.clone())) {
            Some(earlier) => found.push((at, *earlier)),
            None => {
                first.insert((g.ped, g.slot, model, textures), at);
            }
        }
    }
    found
}

/// Garments of one ped and slot that ship the same model bytes and the same texture bytes; equal models with other textures are other garments.
fn duplicates(r: &mut Report, facts: &Facts) {
    let garments = &r.wardrobe.garments;
    let found: Vec<(&Garment, &Garment)> = duplicate_pairs(r.wardrobe, facts)
        .into_iter()
        .map(|(at, earlier)| (&garments[at], &garments[earlier]))
        .collect();
    for (g, earlier) in found {
        let same = match g.variations.iter().any(|v| v.texture.is_some()) {
            true => ", with the same textures",
            false => "",
        };
        r.add(
            "duplicate-content",
            Some(g),
            format!(
                "{} ships the same model as {}{same}",
                name_of(g),
                name_of(earlier)
            ),
        );
    }
}

/// What the files of a replaced ped replace, said once for the ped, and each garment of it that is pinned at no number of the game's.
fn replaced(r: &mut Report, allocation: &Allocation) {
    let wardrobe = r.wardrobe;
    for ped in crate::names::Ped::ALL {
        let Some(what) = crate::replace_mode::replaced(wardrobe, ped) else {
            continue;
        };
        if !allocation.placed.iter().any(|p| p.ped == ped) {
            continue;
        }
        r.add(
            "replace-mode",
            None,
            format!(
                "the files of {ped} replace {what}; {}",
                crate::replace_mode::UNVERIFIED
            ),
        );
    }
    for g in &wardrobe.garments {
        let replaced = g.ped.is_some_and(|ped| wardrobe.replaces(ped).is_some());
        if replaced && g.position.is_none() && !g.is_reserved() {
            r.add(
                "replace-unpinned",
                Some(g),
                format!(
                    "{} replaces the lowest free drawable of its slot, and a replacement is for the drawable of the game it names: pin it at that number",
                    name_of(g)
                ),
            );
        }
    }
}

/// A garment the lock of the last build holds at one drawable and the allocator puts at another, when the pack freezes its drawables.
fn renumbered(r: &mut Report, allocation: &Allocation) {
    let wardrobe = r.wardrobe;
    let Some(lock) = wardrobe.lock.as_ref().filter(|_| wardrobe.rules.freezes()) else {
        return;
    };
    for moved in lock.renumbered(wardrobe, allocation) {
        let garment = &wardrobe.garments[moved.garment];
        let message = crate::freeze::sentence(wardrobe, garment, &moved);
        r.add("renumbered", Some(garment), message);
    }
}

/// A slot past the cap is written as several collections: one finding for the slot, which says how many and what they are called.
fn slot_caps(r: &mut Report, allocation: &Allocation) {
    let cap = r.wardrobe.rules.slot_cap() as usize;
    let mut totals: BTreeMap<(crate::names::Ped, Slot), usize> = BTreeMap::new();
    for p in &allocation.placed {
        if r.wardrobe.replaces(p.ped).is_none() {
            *totals.entry((p.ped, p.slot)).or_default() += 1;
        }
    }
    for ((ped, slot), total) in totals {
        if total <= cap {
            continue;
        }
        let parts = total.div_ceil(cap);
        let names: Vec<String> = (0..parts)
            .map(|n| r.wardrobe.collection_of(ped, n as u32))
            .collect();
        r.add(
            "slot-cap",
            None,
            format!(
                "{slot} of {ped} holds {total} drawables, past the cap of {cap} a collection takes, so a build writes it as {parts} collections: {}",
                names.join(", ")
            ),
        );
    }
}

fn wraps(r: &mut Report, allocation: &Allocation) {
    // numAvailTex is stated by the variation metadata of one collection, so the textures of each collection are counted by themselves.
    let mut totals: BTreeMap<(crate::names::Ped, u32, Slot), usize> = BTreeMap::new();
    for p in &allocation.placed {
        if !p.slot.is_prop() {
            *totals.entry((p.ped, p.part, p.slot)).or_default() +=
                r.wardrobe.garments[p.garment].variations.len();
        }
    }
    for ((ped, part, slot), total) in totals {
        if total >= 256 {
            let of = match part {
                0 => format!("{ped}"),
                n => format!("{ped} in {}", r.wardrobe.collection_of(ped, n)),
            };
            r.add(
                "texture-count-wraps",
                None,
                format!(
                    "{slot} of {of} holds {total} textures, and numAvailTex states {} of them",
                    total % 256
                ),
            );
        }
    }
}

fn alternates(r: &mut Report, allocation: &Allocation) {
    for problem in crate::alternate::problems(r.wardrobe, allocation) {
        let garment = problem
            .garment
            .as_deref()
            .and_then(|id| r.wardrobe.garment(id));
        r.add(problem.code, garment, problem.message);
    }
}

fn first_person(r: &mut Report) {
    for (id, message) in crate::firstperson::problems(r.wardrobe) {
        let garment = r.wardrobe.garment(&id);
        r.add("first-person-alternate", garment, message);
    }
}

fn creature(r: &mut Report, allocation: &Allocation) {
    // One creature file serves a collection of each ped, so a part is a namespace of its own.
    let mut seen: BTreeMap<(u32, Slot, u32, u32), (&Garment, &crate::model::Expression)> =
        BTreeMap::new();
    let mut clashes = Vec::new();
    for p in &allocation.placed {
        let g = &r.wardrobe.garments[p.garment];
        for e in &g.expressions {
            match seen.get(&(p.part, p.slot, p.position, e.index)) {
                Some((other, held)) if *held != e => {
                    clashes.push((g, *other, p.slot, p.position, e.index))
                }
                Some(_) => {}
                None => {
                    seen.insert((p.part, p.slot, p.position, e.index), (g, e));
                }
            }
        }
    }
    for (g, other, slot, position, index) in clashes {
        r.add(
            "creature-clash",
            Some(g),
            format!(
                "{} and {} are both {slot} {position:03}, and state expression {index} differently",
                name_of(g),
                name_of(other)
            ),
        );
    }
}

/// What the pack's rules say that changes nothing: a rule that is not there, one no reason mends, a limit a rule has none of.
fn unheeded(r: &mut Report) {
    for (code, why) in r.wardrobe.rules.unheeded() {
        let said = format!("the rules of the pack set {}, {why}", rule_words(&code));
        r.add("setting-unheeded", None, said);
    }
}

/// A rule as a sentence names it: what it says, or the word a waiver gave where no rule has that word.
fn rule_words(code: &str) -> String {
    match rule_of(code) {
        Some(rule) => format!("the rule \"{}\"", rule.summary),
        None => format!("\"{code}\""),
    }
}

fn waivers(r: &mut Report) {
    let w = r.wardrobe;
    let mut applied: BTreeSet<usize> = BTreeSet::new();
    for (i, waiver) in w.waivers.iter().enumerate() {
        let before = r.found.len();
        match rule_of(&waiver.code) {
            None => r.add(
                "waiver-unknown",
                None,
                format!(
                    "a waiver names {}, which is no rule",
                    rule_words(&waiver.code)
                ),
            ),
            Some(_) if waiver.reason.trim().is_empty() => r.add(
                "waiver-without-reason",
                None,
                format!("the waiver of {} gives no reason", rule_words(&waiver.code)),
            ),
            Some(rule) if !rule.waivable => r.add(
                "waiver-unknown",
                None,
                format!(
                    "{} cannot be waived: no reason mends it",
                    rule_words(&waiver.code)
                ),
            ),
            Some(_) => {
                applied.insert(i);
            }
        }
        for added in &mut r.found[before..] {
            added.waiver = Some(i);
        }
    }
    for finding in &mut r.found {
        let waiver = applied.iter().map(|i| &w.waivers[*i]).find(|waiver| {
            waiver.code == finding.code
                && (waiver.garment.is_none() || waiver.garment == finding.garment)
        });
        if let Some(waiver) = waiver {
            finding.waived = Some(waiver.reason.clone());
        }
    }
}

/// The reason of the waiver that sets rule `code` aside for the garment `garment`, none where no waiver `check` heeds does.
pub(crate) fn waived_for<'a>(wardrobe: &'a Wardrobe, code: &str, garment: &str) -> Option<&'a str> {
    rule_of(code).filter(|rule| rule.waivable)?;
    (wardrobe.waivers.iter())
        .find(|w| {
            w.code == code
                && !w.reason.trim().is_empty()
                && w.garment.as_deref().is_none_or(|g| g == garment)
        })
        .map(|w| w.reason.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc::allocate;
    use crate::model::{Expression, ShopEntry, Stated, Variation, Waiver};
    use crate::names::{Component, Ped};

    const JBIB: Slot = Slot::Component(Component::Jbib);

    fn garment(model: &str, textures: &[&str]) -> Garment {
        let mut g = Garment::new();
        g.ped = Some(Ped::Male);
        g.slot = Some(JBIB);
        g.model = Some(model.into());
        g.variations = textures
            .iter()
            .map(|t| Variation {
                texture: Some((*t).into()),
                ..Default::default()
            })
            .collect();
        g
    }

    fn facts_for(w: &Wardrobe) -> Facts {
        Facts::from_files(crate::facts::named_paths(w).map(|p| (p, 10, p)))
    }

    fn codes(w: &Wardrobe, facts: &Facts) -> Vec<&'static str> {
        check(w, &allocate(w), facts)
            .iter()
            .map(|f| f.code)
            .collect()
    }

    #[test]
    fn a_pack_of_placed_garments_with_their_files_has_nothing_wrong() {
        let mut w = Wardrobe::new("pack");
        w.garments.push(garment("a.ydd", &["a.ytd"]));
        assert_eq!(codes(&w, &facts_for(&w)), Vec::<&str>::new());
    }

    #[test]
    fn every_rule_has_a_row_and_each_code_is_the_one_a_finding_uses() {
        let mut seen = BTreeSet::new();
        for r in RULES {
            assert!(seen.insert(r.code), "{} is listed twice", r.code);
            assert!(!r.summary.is_empty());
            // An error a reason can mend is one that waits for a reader to refuse, a cloth a pack does without, and a renumbering a project chooses.
            assert!(
                !r.waivable
                    || r.severity != Severity::Error
                    || r.code.ends_with("-unreadable")
                    || r.code == "cloth-missing"
                    || r.code == "renumbered",
                "{}",
                r.code
            );
        }
    }

    #[test]
    fn a_file_that_is_missing_a_folder_or_outside_the_module_is_an_error_of_its_own() {
        let mut w = Wardrobe::new("pack");
        w.garments
            .push(garment("gone.ydd", &["gone.ytd", "../up.ytd", "dir.ytd"]));
        let mut facts = Facts::default();
        facts.insert("gone.ydd", Fact::Missing);
        facts.insert("gone.ytd", Fact::Missing);
        facts.insert("../up.ytd", Fact::Escapes);
        facts.insert("dir.ytd", Fact::Folder);
        assert_eq!(
            codes(&w, &facts),
            [
                "model-missing",
                "texture-missing",
                "path-escapes",
                "texture-missing"
            ]
        );
    }

    #[test]
    fn a_garment_with_a_model_needs_a_texture_and_no_more_than_the_letters_allow() {
        let mut w = Wardrobe::new("pack");
        w.garments.push(garment("a.ydd", &[]));
        let many: Vec<String> = (0..27).map(|i| format!("t{i}.ytd")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        w.garments.push(garment("b.ydd", &many));
        let facts = facts_for(&w);
        assert_eq!(codes(&w, &facts), ["variation-none", "variation-cap"]);
    }

    #[test]
    fn a_garment_that_holds_a_place_is_an_info_and_not_a_problem() {
        let mut w = Wardrobe::new("pack");
        let mut g = Garment::new();
        g.ped = Some(Ped::Male);
        g.slot = Some(JBIB);
        w.garments.push(g);
        let found = check(&w, &allocate(&w), &Facts::default());
        assert_eq!(found.len(), 1);
        assert_eq!(
            (found[0].code, found[0].severity),
            ("reserved", Severity::Info)
        );
        assert!(!found[0].blocks());
    }

    #[test]
    fn a_garment_with_no_ped_or_slot_a_clash_and_a_full_slot_are_each_named() {
        let mut w = Wardrobe::new("pack");
        let mut nobody = garment("a.ydd", &["a.ytd"]);
        nobody.ped = None;
        w.garments.push(nobody);
        let mut a = garment("b.ydd", &["b.ytd"]);
        a.position = Some(4);
        let mut b = garment("c.ydd", &["c.ytd"]);
        b.position = Some(4);
        let mut far = garment("d.ydd", &["d.ytd"]);
        far.position = Some(9000);
        w.garments.extend([a, b, far]);
        let facts = facts_for(&w);
        assert_eq!(
            codes(&w, &facts),
            ["garment-unplaced", "position-clash", "slot-full"]
        );
    }

    /// Make the file `path` hold the bytes of `like`, as far as a digest can tell.
    fn same_bytes(facts: &mut Facts, path: &str, like: &str) {
        facts.insert(
            path,
            Fact::File {
                bytes: 10,
                digest: Some(like.into()),
                read: None,
                measured: None,
            },
        );
    }

    #[test]
    fn two_garments_of_one_type_with_one_model_and_the_same_textures_are_a_duplicate_found_by_content(
    ) {
        let mut w = Wardrobe::new("pack");
        w.garments.push(garment("a.ydd", &["a.ytd", "a2.ytd"]));
        w.garments.push(garment("copy.ydd", &["b.ytd", "b2.ytd"]));
        let mut facts = facts_for(&w);
        same_bytes(&mut facts, "copy.ydd", "a.ydd");
        // The same files in another order are the same textures.
        same_bytes(&mut facts, "b.ytd", "a2.ytd");
        same_bytes(&mut facts, "b2.ytd", "a.ytd");
        let found = check(&w, &allocate(&w), &facts);
        assert_eq!(
            found.iter().map(|f| f.code).collect::<Vec<_>>(),
            ["duplicate-content"]
        );
        assert_eq!(found[0].garment.as_deref(), Some(w.garments[1].id.as_str()));
        assert!(
            found[0].message.contains("the same textures"),
            "{}",
            found[0].message
        );
    }

    #[test]
    fn equal_models_with_different_textures_are_not_duplicates() {
        let mut w = Wardrobe::new("pack");
        w.garments.push(garment("a.ydd", &["a.ytd"]));
        w.garments.push(garment("copy.ydd", &["b.ytd"]));
        let mut facts = facts_for(&w);
        same_bytes(&mut facts, "copy.ydd", "a.ydd");
        assert_eq!(codes(&w, &facts), Vec::<&str>::new());

        // One texture more is a different set.
        let mut w = Wardrobe::new("pack");
        w.garments.push(garment("a.ydd", &["a.ytd"]));
        w.garments.push(garment("copy.ydd", &["b.ytd", "c.ytd"]));
        let mut facts = facts_for(&w);
        same_bytes(&mut facts, "copy.ydd", "a.ydd");
        same_bytes(&mut facts, "b.ytd", "a.ytd");
        assert_eq!(codes(&w, &facts), Vec::<&str>::new());
    }

    #[test]
    fn equal_models_and_textures_of_another_ped_or_slot_are_not_duplicates() {
        let mut w = Wardrobe::new("pack");
        w.garments.push(garment("a.ydd", &["a.ytd"]));
        let mut other_ped = garment("copy.ydd", &["b.ytd"]);
        other_ped.ped = Some(Ped::Female);
        let mut other_slot = garment("again.ydd", &["c.ytd"]);
        other_slot.slot = Some(crate::names::Slot::Component(crate::names::Component::Lowr));
        w.garments.extend([other_ped, other_slot]);
        let mut facts = facts_for(&w);
        for (model, texture) in [("copy.ydd", "b.ytd"), ("again.ydd", "c.ytd")] {
            same_bytes(&mut facts, model, "a.ydd");
            same_bytes(&mut facts, texture, "a.ytd");
        }
        assert_eq!(codes(&w, &facts), Vec::<&str>::new());
    }

    #[test]
    fn a_garment_with_a_texture_nobody_could_read_is_not_judged_a_duplicate() {
        let mut w = Wardrobe::new("pack");
        w.garments.push(garment("a.ydd", &["a.ytd"]));
        w.garments.push(garment("copy.ydd", &["gone.ytd"]));
        let mut facts = facts_for(&w);
        same_bytes(&mut facts, "copy.ydd", "a.ydd");
        facts.insert("gone.ytd", Fact::Missing);
        let found = codes(&w, &facts);
        assert!(!found.contains(&"duplicate-content"), "{found:?}");
    }

    #[test]
    fn a_slot_of_256_textures_states_them_modulo_256() {
        let mut w = Wardrobe::new("pack");
        for i in 0..11 {
            let many: Vec<String> = (0..24).map(|t| format!("t{i}_{t}.ytd")).collect();
            let many: Vec<&str> = many.iter().map(String::as_str).collect();
            w.garments.push(garment(&format!("m{i}.ydd"), &many));
        }
        let found = check(&w, &allocate(&w), &facts_for(&w));
        assert_eq!(found.len(), 1, "264 textures: {found:?}");
        assert!(
            found[0].message.contains("264") && found[0].message.contains(" 8 "),
            "{}",
            found[0].message
        );
    }

    #[test]
    fn a_prop_flags_value_must_fit_sixteen_bits_and_expression_arrays_must_agree() {
        let mut w = Wardrobe::new("pack");
        let mut prop = garment("a.ydd", &["a.ytd"]);
        prop.slot = Some(Slot::Prop(crate::names::Prop::Head));
        prop.meta.flags = Some(70_000);
        prop.expressions.push(Expression {
            index: 0,
            tracks: vec![1],
            ids: vec![1, 2],
            types: vec![2],
            components: vec![1],
        });
        w.garments.push(prop);
        assert_eq!(
            codes(&w, &facts_for(&w)),
            ["meta-out-of-range", "expression-arrays"]
        );
    }

    #[test]
    fn two_garments_of_one_place_that_state_an_expression_differently_clash_in_the_one_creature_file(
    ) {
        let mut w = Wardrobe::new("pack");
        let expression = |ids: Vec<u16>| Expression {
            index: 4,
            tracks: vec![33],
            ids,
            types: vec![2],
            components: vec![1],
        };
        let mut male = garment("a.ydd", &["a.ytd"]);
        male.position = Some(3);
        male.expressions.push(expression(vec![28462]));
        let mut female = male.clone();
        female.id = crate::model::new_id();
        female.ped = Some(Ped::Female);
        female.model = Some("b.ydd".into());
        w.garments.extend([male, female.clone()]);
        assert_eq!(
            codes(&w, &facts_for(&w)),
            Vec::<&str>::new(),
            "the same expression is one entry"
        );
        w.garments[1].expressions[0] = expression(vec![1]);
        assert_eq!(codes(&w, &facts_for(&w)), ["creature-clash"]);
    }

    #[test]
    fn a_garment_missing_its_cloth_is_an_error_a_reason_can_set_aside() {
        let mut w = Wardrobe::new("pack");
        let mut cloth = garment("a.ydd", &["a.ytd"]);
        cloth.missing_cloth = true;
        let none = garment("b.ydd", &["b.ytd"]);
        w.garments.extend([cloth, none]);
        let found = check(&w, &allocate(&w), &facts_for(&w));
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            (found[0].code, found[0].severity),
            ("cloth-missing", Severity::Error)
        );
        assert_eq!(found[0].garment.as_deref(), Some(w.garments[0].id.as_str()));
        assert!(found[0].blocks(), "a build stops for it");

        w.waivers.push(Waiver {
            code: "cloth-missing".into(),
            garment: None,
            reason: "the cloth is added by hand afterwards".into(),
        });
        let found = check(&w, &allocate(&w), &facts_for(&w));
        assert_eq!(
            found[0].waived.as_deref(),
            Some("the cloth is added by hand afterwards")
        );
        assert!(!found[0].blocks(), "a reason lets it build");
    }

    #[test]
    fn a_cloth_that_is_linked_satisfies_the_garment_and_one_the_game_reads_none_for_is_ignored() {
        let mut w = Wardrobe::new("pack");
        let mut shipped = garment("a.ydd", &["a.ytd"]);
        shipped.cloth = Some("a.yld".into());
        let mut prop = garment("b.ydd", &["b.ytd"]);
        prop.slot = Some(Slot::Prop(crate::names::Prop::Head));
        prop.cloth = Some("b.yld".into());
        let mut held = Garment::new();
        held.ped = Some(Ped::Male);
        held.slot = Some(Slot::Component(Component::Lowr));
        held.cloth = Some("c.yld".into());
        w.garments.extend([shipped, prop, held]);
        let found = check(&w, &allocate(&w), &facts_for(&w));
        let wording: Vec<_> = found.iter().map(|f| (f.code, f.message.as_str())).collect();
        assert_eq!(
            found.iter().map(|f| f.code).collect::<Vec<_>>(),
            ["cloth-ignored", "reserved", "cloth-ignored"],
            "{wording:?}"
        );
        assert!(
            found[0].message.contains("a prop owns no cloth"),
            "{wording:?}"
        );
        assert!(
            found[2].message.contains("no model for the cloth"),
            "{wording:?}"
        );
        assert!(found[0].garment != Some(w.garments[0].id.clone()));
    }

    #[test]
    fn a_cloth_file_is_held_to_the_same_checks_a_model_is() {
        let mut w = Wardrobe::new("pack");
        let mut g = garment("a.ydd", &["a.ytd"]);
        g.cloth = Some("gone.yld".into());
        w.garments.push(g);
        let mut facts = facts_for(&w);
        facts.insert("gone.yld", Fact::Missing);
        assert_eq!(codes(&w, &facts), ["cloth-file-missing"]);
        facts.insert(
            "gone.yld",
            Fact::File {
                bytes: 3,
                digest: None,
                read: Some(Err("not a cloth dictionary".into())),
                measured: None,
            },
        );
        assert_eq!(codes(&w, &facts), ["cloth-unreadable"]);
        facts.insert("gone.yld", Fact::Escapes);
        assert_eq!(codes(&w, &facts), ["path-escapes"]);
    }

    #[test]
    fn a_garment_with_no_label_or_model_is_named_by_an_id_that_may_hold_wide_characters() {
        // `aéééé` is nine bytes and the eighth is inside the fourth `é`; the long one is cut after eight characters.
        let mut w = Wardrobe::new("pack");
        for id in ["aéééé", "éééééééééé"] {
            let mut g = Garment::new();
            g.id = id.into();
            g.ped = Some(Ped::Male);
            g.slot = Some(JBIB);
            w.garments.push(g);
        }
        assert_eq!(name_of(&w.garments[0]), "the garment aéééé");
        assert_eq!(name_of(&w.garments[1]), "the garment éééééééé");
        assert_eq!(codes(&w, &Facts::default()), ["reserved", "reserved"]);
    }

    #[test]
    fn a_waiver_sets_a_finding_aside_with_its_reason_and_only_the_finding_it_names() {
        let mut w = Wardrobe::new("pack");
        w.garments.push(garment("a.ydd", &["a.ytd"]));
        w.garments.push(garment("copy.ydd", &["b.ytd"]));
        w.garments.push(garment("copy2.ydd", &["c.ytd"]));
        let mut facts = facts_for(&w);
        for (model, texture) in [("copy.ydd", "b.ytd"), ("copy2.ydd", "c.ytd")] {
            same_bytes(&mut facts, model, "a.ydd");
            same_bytes(&mut facts, texture, "a.ytd");
        }
        w.waivers.push(Waiver {
            code: "duplicate-content".into(),
            garment: Some(w.garments[1].id.clone()),
            reason: "the same mesh on purpose".into(),
        });
        let found = check(&w, &allocate(&w), &facts);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].waived.as_deref(), Some("the same mesh on purpose"));
        assert_eq!(found[1].waived, None, "the other copy is not named");

        // A waiver of every garment, and one that gives no reason, an unknown code and an error no reason mends.
        w.waivers.clear();
        w.waivers.push(Waiver {
            code: "duplicate-content".into(),
            garment: None,
            reason: "all".into(),
        });
        assert!(check(&w, &allocate(&w), &facts)
            .iter()
            .all(|f| f.waived.is_some()));
        w.waivers = vec![
            Waiver {
                code: "duplicate-content".into(),
                garment: None,
                reason: " ".into(),
            },
            Waiver {
                code: "no-such-rule".into(),
                garment: None,
                reason: "x".into(),
            },
            Waiver {
                code: "model-missing".into(),
                garment: None,
                reason: "x".into(),
            },
        ];
        let found = check(&w, &allocate(&w), &facts);
        let codes: Vec<_> = found.iter().map(|f| f.code).collect();
        assert_eq!(
            codes,
            [
                "duplicate-content",
                "duplicate-content",
                "waiver-without-reason",
                "waiver-unknown",
                "waiver-unknown"
            ]
        );
        assert!(
            found.iter().all(|f| f.waived.is_none()),
            "none of them took"
        );
        assert!(found.iter().any(Finding::blocks));
    }

    /// Every rule's finding, from packs that each break some: the rules as a person meets them in Problems.
    fn every_rule_broken() -> Vec<Finding> {
        fn unreadable(why: &str) -> Fact {
            Fact::File {
                bytes: 3,
                digest: None,
                read: Some(Err(why.into())),
                measured: None,
            }
        }
        let mut found = Vec::new();

        let w = Wardrobe::new("Not A Name");
        found.extend(check(&w, &allocate(&w), &Facts::default()));

        // The seat of a garment, and its files.
        let mut w = Wardrobe::new("pack");
        let mut nobody = garment("nobody.ydd", &["nobody.ytd"]);
        nobody.ped = None;
        let (mut a, mut b) = (garment("a.ydd", &["a.ytd"]), garment("b.ydd", &["b.ytd"]));
        (a.position, b.position) = (Some(4), Some(4));
        let mut far = garment("far.ydd", &["far.ytd"]);
        far.position = Some(9000);
        let mut held = Garment::new();
        held.ped = Some(Ped::Male);
        held.slot = Some(JBIB);
        let many: Vec<String> = (0..27).map(|i| format!("t{i}.ytd")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        let mut gone = garment("gone.ydd", &["gone.ytd", "../up.ytd", "dir.ytd", "bad.ytd"]);
        gone.cloth = Some("gone.yld".into());
        let mut broken = garment("broken.ydd", &["fine.ytd"]);
        broken.cloth = Some("broken.yld".into());
        let mut wanting = garment("wanting.ydd", &["wanting.ytd"]);
        wanting.missing_cloth = true;
        let mut prop = garment("prop.ydd", &["prop.ytd"]);
        prop.slot = Some(Slot::Prop(crate::names::Prop::Head));
        prop.cloth = Some("prop.yld".into());
        prop.meta.flags = Some(70_000);
        prop.expressions.push(Expression {
            index: 0,
            tracks: vec![1],
            ids: vec![1, 2],
            types: vec![2],
            components: vec![1],
        });
        w.garments.extend([
            nobody,
            a,
            b,
            far,
            held,
            garment("none.ydd", &[]),
            garment("many.ydd", &many),
            gone,
            broken,
            wanting,
            prop,
            garment("twin.ydd", &["twin.ytd"]),
            garment("twin_copy.ydd", &["twin_copy.ytd"]),
        ]);
        w.waivers = vec![
            Waiver {
                code: "duplicate-content".into(),
                garment: None,
                reason: " ".into(),
            },
            Waiver {
                code: "no-such-rule".into(),
                garment: None,
                reason: "x".into(),
            },
            Waiver {
                code: "model-missing".into(),
                garment: None,
                reason: "x".into(),
            },
        ];
        // A shop entry two textures write, and one that points at no garment.
        for g in &mut w.garments[11..] {
            g.variations[0].shop_id = Some("SAME_ID".into());
        }
        w.shop.unplaced.push(Stated {
            ped: Ped::Male,
            kind: "PV_COMP_JBIB".into(),
            prop: false,
            drawable: 99,
            texture: 0,
            entry: ShopEntry {
                id: "STRAY".into(),
                from: "shop.meta".into(),
                entry: 0,
                ..ShopEntry::default()
            },
        });
        let mut facts = facts_for(&w);
        facts.insert("gone.ydd", Fact::Missing);
        facts.insert("gone.ytd", Fact::Missing);
        facts.insert("../up.ytd", Fact::Escapes);
        facts.insert("dir.ytd", Fact::Folder);
        facts.insert("bad.ytd", unreadable("not a texture dictionary"));
        facts.insert("broken.yld", unreadable("not a cloth dictionary"));
        facts.insert("gone.yld", Fact::Missing);
        facts.insert("broken.ydd", unreadable("not a drawable"));
        for file in ["twin.ydd", "twin_copy.ydd", "twin.ytd", "twin_copy.ytd"] {
            same_bytes(&mut facts, file, &file.replace("_copy", ""));
        }
        found.extend(check(&w, &allocate(&w), &facts));

        // Files that state more than their limits allow, and a setting that changes nothing.
        {
            use crate::measure::{Drawable, Lod, Measure, Texture};
            let tex = |name: &str, side: u32, mips: u32, format: &str| Texture {
                name: name.into(),
                width: side,
                height: side,
                mips,
                format: format.into(),
            };
            let counted = |facts: &mut Facts, path: &str, measure: Measure| {
                facts.insert(
                    path,
                    Fact::File {
                        bytes: 10,
                        digest: Some(path.into()),
                        read: None,
                        measured: serde_json::to_value(measure).ok(),
                    },
                );
            };
            let lod = |vertices, triangles| {
                Some(Lod {
                    vertices,
                    triangles,
                })
            };
            let dictionary = |textures: Vec<Texture>, streaming: u64| Measure {
                streaming,
                detail: true,
                enhanced: false,
                textures,
                drawables: Vec::new(),
            };
            let mut w = Wardrobe::new("pack");
            let dictionaries = [
                ("size.ytd", dictionary(vec![tex("a", 4096, 13, "DXT1")], 1)),
                ("odd.ytd", dictionary(vec![tex("b", 1000, 10, "DXT1")], 1)),
                ("flat.ytd", dictionary(vec![tex("c", 1024, 1, "DXT1")], 1)),
                (
                    "raw.ytd",
                    dictionary(vec![tex("d", 1024, 11, "A8R8G8B8")], 1),
                ),
                ("seven.ytd", dictionary(vec![tex("e", 1024, 11, "BC7")], 1)),
                (
                    "tone.ytd",
                    dictionary(vec![tex("jbib_diff_000_a_whi", 256, 9, "DXT1")], 1),
                ),
                (
                    "heavy.ytd",
                    dictionary(vec![tex("f", 1024, 11, "DXT1")], 1 << 30),
                ),
            ];
            let names: Vec<&str> = dictionaries.iter().map(|(path, _)| *path).collect();
            let mut g = garment("measured.ydd", &names);
            g.skin = crate::names::Skin::Race;
            w.garments.push(g);
            let drawable =
                |lods: [Option<Lod>; 4], without: Vec<u32>, stray: Vec<String>| Drawable {
                    name: "model".into(),
                    lods,
                    shaders: 2,
                    without_diffuse: without,
                    unused_embedded: stray,
                    missing_parameters: vec![crate::measure::Missing {
                        shader: 1,
                        names: vec!["bumpiness".into()],
                    }],
                };
            let model = Measure {
                streaming: 1,
                detail: true,
                enhanced: false,
                textures: Vec::new(),
                drawables: vec![
                    drawable(
                        [
                            lod(60_000, 90_000),
                            lod(50_000, 70_000),
                            lod(900, 1000),
                            None,
                        ],
                        vec![1],
                        vec!["stray".into()],
                    ),
                    drawable(
                        [lod(100, 100), None, lod(10, 10), None],
                        Vec::new(),
                        Vec::new(),
                    ),
                ],
            };
            let mut facts = Facts::default();
            counted(&mut facts, "measured.ydd", model);
            for (path, measure) in dictionaries {
                counted(&mut facts, path, measure);
            }
            w.rules.settings.insert(
                "no-such-rule".into(),
                crate::settings::Setting {
                    on: Some(false),
                    ..Default::default()
                },
            );
            found.extend(check(&w, &allocate(&w), &facts));
        }

        // A slot of 256 textures.
        let mut w = Wardrobe::new("pack");
        for i in 0..11 {
            let many: Vec<String> = (0..24).map(|t| format!("t{i}_{t}.ytd")).collect();
            let many: Vec<&str> = many.iter().map(String::as_str).collect();
            w.garments.push(garment(&format!("m{i}.ydd"), &many));
        }
        found.extend(check(&w, &allocate(&w), &facts_for(&w)));

        // Two garments of one place that state an expression differently.
        let mut w = Wardrobe::new("pack");
        let state = |ids: Vec<u16>| Expression {
            index: 4,
            tracks: vec![33],
            ids,
            types: vec![2],
            components: vec![1],
        };
        let mut male = garment("male.ydd", &["male.ytd"]);
        male.position = Some(3);
        male.expressions.push(state(vec![28462]));
        let mut female = garment("female.ydd", &["female.ytd"]);
        (female.ped, female.position) = (Some(Ped::Female), Some(3));
        female.expressions.push(state(vec![1]));
        w.garments.extend([male, female]);
        found.extend(check(&w, &allocate(&w), &facts_for(&w)));

        // A mask that wears a first person alternate it has no model for.
        let mut w = Wardrobe::new("pack");
        let mut mask = garment("mask.ydd", &["mask.ytd"]);
        mask.slot = Some(Slot::Component(crate::names::Component::Berd));
        mask.meta.first_person_alternate = Some(2);
        w.garments.push(mask);
        found.extend(check(&w, &allocate(&w), &facts_for(&w)));

        // A set that names a garment the pack lacks, and a switch to an alternate nothing has.
        let mut w = Wardrobe::new("pack");
        let hair = garment("hair.ydd", &["hair.ytd"]);
        let hair_id = hair.id.clone();
        w.garments.push(hair);
        let set = crate::alternate::new_set(&mut w, Ped::Male, "hood").unwrap();
        crate::alternate::add_switch(&mut w, &set, crate::alternate::Asset::garment(&hair_id), 1)
            .unwrap();
        w.alternates[0]
            .sources
            .push(crate::alternate::Asset::garment("nobody"));
        found.extend(check(&w, &allocate(&w), &facts_for(&w)));
        let mut w = Wardrobe::new("pack");
        let set = crate::alternate::new_set(&mut w, Ped::Male, "heavy").unwrap();
        let mut top = garment("top.ydd", &["top.ytd"]);
        top.position = Some(0);
        let top_id = top.id.clone();
        w.garments.push(top);
        crate::alternate::join(&mut w, &set, crate::alternate::Asset::garment(&top_id)).unwrap();
        for i in 0..=crate::alternate::MOST_FOR_ONE_ASSET {
            let place = u32::try_from(i % 200).unwrap();
            let alt = 1 + u8::try_from(i / 200).unwrap();
            crate::alternate::add_switch(
                &mut w,
                &set,
                crate::alternate::Asset::outside(
                    None,
                    Slot::Component(crate::names::Component::Hair),
                    place,
                ),
                alt,
            )
            .unwrap();
        }
        found.extend(check(&w, &allocate(&w), &facts_for(&w)));

        // A slot past the cap of one collection.
        let mut w = Wardrobe::new("pack");
        w.garments = (0..130)
            .map(|i| garment(&format!("g{i}.ydd"), &["t.ytd"]))
            .collect();
        found.extend(check(&w, &allocate(&w), &Facts::default()));

        // A garment the last build shipped at one drawable and the allocator puts at another.
        let mut w = Wardrobe::new("pack");
        w.garments = vec![garment("moved.ydd", &["moved.ytd"])];
        let mut lock = crate::freeze::Lock::of(&w, &allocate(&w));
        lock.places[0].position = 5;
        w.lock = Some(lock);
        found.extend(check(&w, &allocate(&w), &Facts::default()));

        // The files of a ped that replace the game's own, for a garment pinned at no number of the game's.
        let mut w = Wardrobe::new("pack");
        w.rules
            .replace
            .insert(Ped::Male, crate::settings::Replace::default());
        w.garments = vec![garment("replacing.ydd", &["replacing.ytd"])];
        found.extend(check(&w, &allocate(&w), &Facts::default()));

        // A drawable of the race skin whose propMask asks for the universal form of its files.
        let mut w = Wardrobe::new("pack");
        let mut race = garment("race.ydd", &["race.ytd"]);
        race.skin = crate::names::Skin::Race;
        race.meta.prop_mask = Some(1);
        w.garments.push(race);
        found.extend(check(&w, &allocate(&w), &facts_for(&w)));
        found
    }

    /// The file names a sentence says twice, which is a name that a person reads twice.
    fn names_said_twice(sentence: &str) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        let mut twice = Vec::new();
        for word in sentence.split(|c: char| c.is_whitespace() || ",:()\"".contains(c)) {
            let name = word.rsplit('/').next().unwrap_or(word);
            if name.contains('.') && !name.ends_with('.') {
                if seen.iter().any(|s| s == name) {
                    twice.push(name.to_string());
                }
                seen.push(name.to_string());
            }
        }
        twice
    }

    #[test]
    fn every_rule_says_what_is_wrong_in_a_plain_sentence() {
        let found = every_rule_broken();
        let seen: BTreeSet<&str> = found.iter().map(|f| f.code).collect();
        let every: BTreeSet<&str> = RULES.iter().map(|r| r.code).collect();
        assert_eq!(
            seen, every,
            "a rule this does not break is a sentence nobody read"
        );
        for finding in &found {
            for sentence in [finding.message.clone(), crate::plan::problem(finding)] {
                let who = format!("{} {sentence:?}", finding.code);
                assert!(!sentence.contains('`'), "a backtick: {who}");
                assert!(
                    !RULES.iter().any(|r| sentence.starts_with(r.code)),
                    "a rule code in front: {who}"
                );
                for word in ["module", "output"] {
                    assert!(
                        !sentence.to_lowercase().contains(word),
                        "the word {word}: {who}"
                    );
                }
                assert_eq!(
                    names_said_twice(&sentence),
                    Vec::<String>::new(),
                    "a name said twice: {who}"
                );
            }
        }
    }
}
