//! What the Problems panel lists: what a scan, a recook or a plan found wrong, and what a person can do about it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use or_plugin::Level;

use super::builds::Builds;
use super::cook::{Cook, Cooks};
use super::leaf;
use super::originals::Originals;
use super::scan::{AssetRow, Kept, Scan, Seen};
use super::wardrobe::Pack;
use crate::state::plugins::extensions::Said;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Warning => tr!("Warning"),
            Self::Error => tr!("Error"),
        }
    }
}

/// What a finding concerns, as the Content and Builds panel counts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// The project file itself.
    Project,
    /// A build, by its id.
    Build(String),
    /// A content as a whole, by its id.
    Content(String),
    /// An asset, by its row's key, and the content that holds it.
    Asset { key: String, content: String },
    /// A garment of a clothing content, by its stable id and its name in a sentence; none for the pack as a whole.
    Garment {
        content: String,
        id: Option<String>,
        label: String,
    },
}

/// What a garment names that is not there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    /// A texture, by its index among the garment's textures and the file it names, so that a row drawn before an edit moved the textures is refused and not applied to another.
    Texture(usize, String),
    Cloth,
    /// The model, which a reference may lead nowhere from.
    Model,
}

impl Missing {
    /// What the button that takes it off the garment says: the thing it takes off, so it is not read as taking the garment off.
    pub fn remove_label(&self) -> &'static str {
        match self {
            Self::Texture(..) => tr!("Remove this texture"),
            Self::Cloth => tr!("Remove the cloth"),
            Self::Model => tr!("Remove the model"),
        }
    }
}

/// Something a person can do about a finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fix {
    /// Point the project at where the file is now.
    Locate(PathBuf),
    /// Take the file out of the project; it stays on disk.
    Remove(PathBuf),
    /// Give the source folder an id of its own, which a folder copied by hand needs.
    Reid(PathBuf),
    /// Point a garment at the file its texture or cloth is in now.
    FindFile {
        module: String,
        garment: String,
        what: Missing,
    },
    /// Take the texture or cloth that is not there off the garment.
    DropFile {
        module: String,
        garment: String,
        what: Missing,
    },
    /// Apply the engine's remedy for a finding of a clothing content, after showing what it changes.
    Remedy {
        module: String,
        finding: or_wardrobe::Finding,
    },
    /// Record why a finding of a clothing content stands.
    Waive {
        module: String,
        finding: or_wardrobe::Finding,
    },
    /// Repair the model of a garment where its counts contradict their runs, losslessly, and say why it cannot where it cannot.
    Repair { module: String, garment: String },
    /// Record why a finding of a validator plugin stands, by the code the plugin states.
    WaivePlugin { code: String },
    /// Write the files a validator plugin suggests with its finding, after showing which they are.
    Plugin {
        /// The plugin's name, which the question puts first.
        name: String,
        code: String,
        /// What the finding says, without its code.
        message: String,
        fix: or_plugin::SuggestedFix,
    },
    /// State the constants a shader declares that a material of the garment's models lacks.
    Materials { module: String, garment: String },
}

impl Fix {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Locate(_) | Self::FindFile { .. } => tr!("Locate…"),
            // ⚠ Two things, so two names: one takes a file out of the project, the other takes what a garment names that is not there off it, and says which.
            Self::Remove(_) => tr!("Remove from project"),
            Self::DropFile { what, .. } => what.remove_label(),
            Self::Reid(_) => tr!("New id"),
            Self::Remedy { .. } | Self::Plugin { .. } => tr!("Fix…"),
            Self::Waive { .. } | Self::WaivePlugin { .. } => tr!("Waive…"),
            Self::Repair { .. } => tr!("Repair"),
            Self::Materials { .. } => tr!("Complete"),
        }
    }

    /// What the button does, in a sentence for its hover.
    pub fn hover(&self) -> &'static str {
        match self {
            Self::Locate(_) => tr!("Point the project at the file where it is now"),
            Self::FindFile { .. } => tr!("Point the garment at the file where it is now"),
            Self::Remove(_) => {
                tr!("Take the file out of the project. The file stays on disk, and Undo puts it back")
            }
            Self::DropFile { what, .. } => match what {
                Missing::Texture(..) => {
                    tr!("Take this texture, whose file is not there, off the garment. The garment stays, and Undo takes it back")
                }
                Missing::Cloth => {
                    tr!("Take the cloth, whose file is not there, off the garment. The garment stays, and Undo takes it back")
                }
                Missing::Model => tr!("A garment needs a model, so it cannot be taken off"),
            },
            Self::Reid(_) => {
                tr!("Give this source folder an id of its own, so it and its copy stop sharing one")
            }
            Self::Remedy { .. } => tr!("See what this fix changes, then apply it"),
            Self::Plugin { .. } => tr!("See which files this fix writes, then apply it"),
            Self::Waive { .. } | Self::WaivePlugin { .. } => {
                tr!("Keep this problem and write down why")
            }
            Self::Repair { .. } => {
                tr!("Rewrite the model so its counts agree with its runs, keeping its geometry")
            }
            Self::Materials { .. } => {
                tr!("State the constants the shader declares that a material lacks")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    pub place: Place,
    pub text: String,
    pub fixes: Vec<Fix>,
    /// The rule of the clothing engine that found it, for a finding of a clothing content.
    pub code: Option<&'static str>,
}

impl Finding {
    fn new(severity: Severity, place: Place, text: String) -> Self {
        Self {
            severity,
            place,
            text,
            fixes: Vec::new(),
            code: None,
        }
    }

    /// The same finding with the fix the plugin suggested with `said`, when it did, and a way to let it stand, when it states the code of a validator plugin's finding.
    ///
    /// ⚠ `said` is the plugin's finding this one came from, never one that merely reads the same: two packs can state one sentence, each with a fix of its own.
    fn with_plugin(mut self, said: Option<&Said>) -> Self {
        if let Some(code) = crate::state::plugins::waivers::code_of(&self.text) {
            if let Some((name, fix)) = said.and_then(|s| Some((s.name.clone(), s.fix.clone()?))) {
                let message = (self.text.strip_prefix(&format!("{code}: ")))
                    .unwrap_or(&self.text)
                    .to_string();
                self.fixes.push(Fix::Plugin {
                    name,
                    code: code.clone(),
                    message,
                    fix,
                });
            }
            self.fixes.push(Fix::WaivePlugin { code });
        }
        self
    }

    /// What the finding is about, in words.
    pub fn about(&self) -> String {
        match &self.place {
            Place::Project => tr!("Project").to_string(),
            Place::Build(id) => tr!("Build {id}", id = id),
            Place::Content(content) => content.clone(),
            Place::Garment { label, .. } => label.clone(),
            Place::Asset { key, .. } => std::path::Path::new(key)
                .file_name()
                .map_or_else(|| key.clone(), |n| n.to_string_lossy().into_owned()),
        }
    }
}

/// Every finding of the project as the scan read it, newest information first: errors, then warnings.
pub fn collect(
    scan: &Scan,
    cooks: Option<&Cooks>,
    unsaved: &HashSet<PathBuf>,
    from_file: &[String],
    builds: &Builds,
) -> Vec<Finding> {
    let mut found = Vec::new();
    for row in &scan.rows {
        asset(row, &scan.project, cooks, unsaved, &mut found);
    }
    shared_ids(scan, &mut found);
    for planned in &scan.plans {
        for plan in planned.resources.iter().map(|r| &r.plan) {
            // ⚠ A plugin's errors join the problems of its plan in the order it said them, so the nth problem of a text is the nth error of that text said of this plan.
            let mut seen: HashMap<&str, usize> = HashMap::new();
            for problem in &plan.problems {
                let nth = seen.entry(problem).or_default();
                let said = (scan.said.iter())
                    .filter(|s| s.level == Level::Error && s.text == *problem)
                    .filter(|s| s.output == plan.output && s.resource == plan.resource)
                    .nth(*nth);
                *nth += 1;
                if !said_elsewhere(scan, problem) {
                    found.push(
                        Finding::new(
                            Severity::Error,
                            Place::Build(planned.output.clone()),
                            problem.clone(),
                        )
                        .with_plugin(said),
                    );
                }
            }
        }
    }
    // A plugin's error is already a problem of the plan, listed above; its warnings are only said here.
    for said in scan.said.iter().filter(|s| s.level == Level::Warning) {
        found.push(
            Finding::new(
                Severity::Warning,
                Place::Build(said.output.clone()),
                said.text.clone(),
            )
            .with_plugin(Some(said)),
        );
    }
    for (output, reasons) in builds.failed() {
        failed_build(scan, output, reasons, &mut found);
    }
    for link in &scan.links {
        if let Some(problem) = &link.problem {
            found.push(Finding::new(
                Severity::Error,
                Place::Content(link.module.clone()),
                problem.clone(),
            ));
        }
    }
    for pack in &scan.packs {
        pack_findings(pack, &mut found);
    }
    for particles in &scan.particles {
        particle_findings(particles, &mut found);
    }
    for weapons in &scan.weapons {
        weapon_findings(weapons, &mut found);
    }
    for maps in &scan.maps {
        map_findings(maps, &mut found);
    }
    for text in from_file {
        found.push(Finding::new(
            Severity::Warning,
            Place::Project,
            text.clone(),
        ));
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.severity));
    found
}

/// Sources that share an id: a folder copied by hand keeps its id, and the two would cook into one slot.
///
/// ⚠ Every holder is a row of its own with New id, and none is called the copy: nothing in the folders says which is
/// (the table sorts the pair by folder name, so `logo copy` lists before `logo`). One New id on any holder clears the problem.
/// Their origins tell what happened: the same origin is one file kept twice, another origin is files that were given one id.
fn shared_ids(scan: &Scan, found: &mut Vec<Finding>) {
    let mut at: HashMap<&str, usize> = HashMap::new();
    let mut groups: Vec<Vec<&AssetRow>> = Vec::new();
    for row in scan.rows.iter().filter(|row| row.kept == Kept::Source) {
        let Some(id) = row.id.as_deref() else {
            continue;
        };
        let group = *at.entry(id).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[group].push(row);
    }
    for group in groups.iter().filter(|group| group.len() > 1) {
        let names: Vec<String> = group.iter().map(|row| leaf(&row.path)).collect();
        let (last, rest) = names.split_last().expect("a group holds sources");
        let who = format!("{} and {last}", rest.join(", "));
        let all = if group.len() == 2 { "both" } else { "all" };
        let text = if group.iter().all(|row| row.origin == group[0].origin) {
            tr!("{who} come from the same file and have one id, so a cook would write one output for {all}. Give one of them a new id", who = who, all = all)
        } else {
            tr!("{who} have one id though they do not come from one file: an id belongs to one asset, so a cook would write one output for {all}. Give one of them a new id", who = who, all = all)
        };
        for row in group {
            let place = Place::Asset {
                key: row.key.clone(),
                content: row.content.clone(),
            };
            let mut finding = Finding::new(Severity::Error, place, text.clone());
            finding.fixes.push(Fix::Reid(row.path.clone()));
            found.push(finding);
        }
    }
}

/// The sources whose check against the game found the game's file to be another one now, each a warning: the source still builds as it is.
pub fn original_changes(scan: &Scan, originals: &Originals) -> Vec<Finding> {
    let changed = scan.rows.iter().filter(|row| row.kept == Kept::Source);
    changed
        .filter_map(|row| {
            let verdict = originals.verdict_of(&row.path).filter(|v| v.is_changed())?;
            let place = Place::Asset {
                key: row.key.clone(),
                content: row.content.clone(),
            };
            let text = format!("{}: {}", row.name, verdict.sentence());
            Some(Finding::new(Severity::Warning, place, text))
        })
        .collect()
}

/// The last build of `output` stopped: one row, with the first reason the plan does not already say and how many there are, or that the plan's problems listed here are why.
///
/// ⚠ One row, not one for each reason: a build can stop for every file it wrote. The Build tab lists them.
fn failed_build(scan: &Scan, output: &str, reasons: &[String], found: &mut Vec<Finding>) {
    let place = Place::Build(output.to_string());
    let new: Vec<&String> = (reasons.iter())
        .filter(|reason| !said_elsewhere(scan, reason))
        .filter(|reason| !found.iter().any(|f| f.place == place && f.text == **reason))
        .collect();
    let text = match new.as_slice() {
        [] => tr!("The last build was refused for the problems of its plan that are listed here")
            .to_string(),
        [only] => tr!("The last build stopped: {only}", only = only),
        [first, ..] => tr!(
            "The last build stopped for {count} reasons, the first: {first}",
            first = first,
            count = new.len()
        ),
    };
    found.push(Finding::new(Severity::Error, place, text));
}

/// Whether a row or a clothing content already says `problem` of a plan, once, with what to do about it.
///
/// ⚠ A missing file is said by its row, with Locate… and Remove.
fn said_elsewhere(scan: &Scan, problem: &str) -> bool {
    // ⚠ The plan names a file as the project does, by its place in the project folder, and a file outside it by its path.
    let folder = or_project::project_folder(&scan.project);
    let said_by_a_row = scan.rows.iter().any(|row| {
        let name = or_project::relative_inside(&row.path, &folder)
            .unwrap_or_else(|| row.path.display().to_string());
        matches!(row.seen, Seen::Missing | Seen::Folder)
            && problem.starts_with(&tr!("`{name}` is ", name = name))
    });
    said_by_a_row || said_by_a_pack(scan, problem)
}

/// Whether a clothing or weapons content already says `problem`, once, with what to do about it.
fn said_by_a_pack(scan: &Scan, problem: &str) -> bool {
    let clothing = scan.packs.iter().any(|pack| match &pack.read {
        Err(why) => problem == why,
        Ok(read) => (read.findings.iter()).any(|f| or_wardrobe::plan::problem(f) == problem),
    });
    clothing
        || scan
            .particles
            .iter()
            .any(|particles| match &particles.read {
                Err(why) => problem == why,
                Ok(read) => {
                    (read.findings.iter()).any(|f| or_particles::plan::problem(f) == problem)
                }
            })
        || scan.weapons.iter().any(|weapons| match &weapons.read {
            Err(why) => problem == why,
            Ok(read) => (read.findings.iter()).any(|f| or_weapons::plan::problem(f) == problem),
        })
        || scan.maps.iter().any(|maps| match &maps.read {
            Err(why) => problem == why,
            Ok(read) => (read.findings.iter()).any(|f| or_maps::plan::problem(f) == problem),
        })
}

/// What the engine found in a maps content: each finding that is not waived, an information not counted as a problem.
fn map_findings(maps: &super::MapsRead, found: &mut Vec<Finding>) {
    let place = Place::Content(maps.module.clone());
    let read = match &maps.read {
        Ok(read) => read,
        Err(why) => {
            found.push(Finding::new(Severity::Error, place, why.clone()));
            return;
        }
    };
    for f in read.findings.iter().filter(|f| f.waived.is_none()) {
        let severity = match f.severity {
            or_maps::Severity::Error => Severity::Error,
            or_maps::Severity::Warning => Severity::Warning,
            or_maps::Severity::Info => continue,
        };
        let mut finding = Finding::new(severity, place.clone(), f.message.clone());
        finding.code = Some(f.code);
        found.push(finding);
    }
}

/// What the engine found in a weapons content: each finding that is not waived, an information not counted as a problem.
fn weapon_findings(weapons: &super::WeaponsRead, found: &mut Vec<Finding>) {
    let place = Place::Content(weapons.module.clone());
    let read = match &weapons.read {
        Ok(read) => read,
        Err(why) => {
            found.push(Finding::new(Severity::Error, place, why.clone()));
            return;
        }
    };
    for f in read.findings.iter().filter(|f| f.waived.is_none()) {
        let severity = match f.severity {
            or_weapons::Severity::Error => Severity::Error,
            or_weapons::Severity::Warning => Severity::Warning,
            or_weapons::Severity::Info => continue,
        };
        let mut finding = Finding::new(severity, place.clone(), f.message.clone());
        finding.code = Some(f.code);
        found.push(finding);
    }
}

/// What the engine found in a particles content: each finding that is not waived, an information not counted as a problem.
fn particle_findings(particles: &super::ParticlesRead, found: &mut Vec<Finding>) {
    let place = Place::Content(particles.module.clone());
    let read = match &particles.read {
        Ok(read) => read,
        Err(why) => {
            found.push(Finding::new(Severity::Error, place, why.clone()));
            return;
        }
    };
    for f in read.findings.iter().filter(|f| f.waived.is_none()) {
        let severity = match f.severity {
            or_particles::Severity::Error => Severity::Error,
            or_particles::Severity::Warning => Severity::Warning,
            or_particles::Severity::Info => continue,
        };
        let mut finding = Finding::new(severity, place.clone(), f.message.clone());
        finding.code = Some(f.code);
        found.push(finding);
    }
}

/// What the engine found in a clothing content, each with the fix and the waiver it offers.
///
/// ⚠ A finding a pack has waived is set aside and not listed, and a note is not a problem.
fn pack_findings(pack: &Pack, found: &mut Vec<Finding>) {
    let module = pack.module.clone();
    let read = match &pack.read {
        Ok(read) => read,
        Err(why) => {
            let place = Place::Garment {
                content: module.clone(),
                id: None,
                label: tr!("Clothing {module}", module = module),
            };
            found.push(Finding::new(Severity::Error, place, why.clone()));
            return;
        }
    };
    for f in &read.findings {
        let severity = match f.severity {
            or_wardrobe::Severity::Error => Severity::Error,
            or_wardrobe::Severity::Warning => Severity::Warning,
            or_wardrobe::Severity::Info => continue,
        };
        if f.waived.is_some() {
            continue;
        }
        let garment = f
            .garment
            .as_deref()
            .and_then(|id| read.wardrobe.garment(id));
        let label = garment.map_or_else(
            || tr!("Pack {pack}", pack = read.wardrobe.pack),
            or_wardrobe::rules::name_of,
        );
        let place = Place::Garment {
            content: module.clone(),
            id: garment.map(|g| g.id.clone()),
            label: label.clone(),
        };
        // The row says whose problem it is, so the sentence does not begin with the name again.
        let text = without_subject(&or_wardrobe::plan::problem(f), &label);
        let mut finding = Finding::new(severity, place, text);
        finding.code = Some(f.code);
        if let (false, Some(garment)) = (pack.linked, garment) {
            let missing = match (f.code, f.texture) {
                ("texture-missing", Some(i)) => {
                    let named = garment.variations.get(i).and_then(|v| v.texture.clone());
                    named.map(|file| Missing::Texture(i, file))
                }
                ("cloth-file-missing" | "cloth-missing", _) => Some(Missing::Cloth),
                ("model-missing", _) => Some(Missing::Model),
                _ => None,
            };
            if let Some(what) = missing {
                let (module, garment) = (module.clone(), garment.id.clone());
                finding.fixes.push(Fix::FindFile {
                    module: module.clone(),
                    garment: garment.clone(),
                    what: what.clone(),
                });
                // A cloth the metadata expects and none is linked has its own, milder remedy, so Remove is for the files that are named and not there.
                if f.code != "cloth-missing" && what != Missing::Model {
                    finding.fixes.push(Fix::DropFile {
                        module,
                        garment,
                        what,
                    });
                }
            }
        }
        if let (false, "model-unreadable", Some(garment)) = (pack.linked, f.code, garment) {
            finding.fixes.push(Fix::Repair {
                module: module.clone(),
                garment: garment.id.clone(),
            });
        }
        if let (false, "material-parameters", Some(garment)) = (pack.linked, f.code, garment) {
            finding.fixes.push(Fix::Materials {
                module: module.clone(),
                garment: garment.id.clone(),
            });
        }
        if !pack.linked && or_wardrobe::edit::remedy(&read.wardrobe, f).is_some() {
            finding.fixes.push(Fix::Remedy {
                module: module.clone(),
                finding: f.clone(),
            });
        }
        if !pack.linked && or_wardrobe::rule_of(f.code).is_some_and(|r| r.waivable) {
            finding.fixes.push(Fix::Waive {
                module: module.clone(),
                finding: f.clone(),
            });
        }
        found.push(finding);
    }
}

/// `sentence` without the `name` it begins with, or its possessive, so that a row that names its subject does not say it twice: "my cap has no texture" is "Has no texture" under my cap, and "my cap's model has no low level" is "Model has no low level".
pub(super) fn without_subject(sentence: &str, name: &str) -> String {
    let Some(rest) = (sentence.strip_prefix(name))
        .and_then(|rest| rest.strip_prefix(' ').or_else(|| rest.strip_prefix("'s ")))
    else {
        return sentence.to_string();
    };
    let mut chars = rest.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => sentence.to_string(),
    }
}

/// `path` by its place in the project of `project_file`, with `/`; a path outside the project is its own place.
fn place_in_project(project_file: &Path, path: &Path) -> String {
    match path.strip_prefix(or_project::project_folder(project_file)) {
        Ok(inside) => inside.to_string_lossy().replace('\\', "/"),
        Err(_) => path.display().to_string(),
    }
}

fn asset(
    row: &AssetRow,
    project_file: &Path,
    cooks: Option<&Cooks>,
    unsaved: &HashSet<PathBuf>,
    found: &mut Vec<Finding>,
) {
    let place = Place::Asset {
        key: row.key.clone(),
        content: row.content.clone(),
    };
    let mut error = |text: String, fixes: Vec<Fix>| {
        found.push(Finding {
            severity: Severity::Error,
            place: place.clone(),
            text,
            fixes,
            code: None,
        });
    };
    match &row.seen {
        Seen::Missing => error(
            tr!(
                "{name} is missing: {path}",
                name = row.name,
                path = place_in_project(project_file, &row.path)
            ),
            vec![Fix::Locate(row.path.clone()), Fix::Remove(row.path.clone())],
        ),
        Seen::Folder => error(
            tr!(
                "{name} is a folder, and the Files content holds files",
                name = row.name
            ),
            vec![Fix::Remove(row.path.clone())],
        ),
        Seen::Unreadable(why) => error(why.clone(), Vec::new()),
        Seen::Present | Seen::NotCooked | Seen::Cooked => {}
    }
    if row.kept == Kept::Source {
        if let Some(Cook::Failed(problems)) = cooks.and_then(|c| c.state(&row.path)) {
            for problem in problems {
                error(format!("{}: {problem}", row.name), Vec::new());
            }
        }
    }
    if row.kept == Kept::File && row.seen == Seen::Present && unsaved.contains(&row.path) {
        found.push(Finding::new(
            Severity::Warning,
            Place::Asset {
                key: row.key.clone(),
                content: row.content.clone(),
            },
            tr!(
                "{name} has unsaved changes, which a build does not include",
                name = row.name
            ),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::workbench::scan::Seen;

    fn a_source(name: &str, id: &str, origin: &str) -> AssetRow {
        AssetRow {
            key: format!("sources/{name}.textures.src"),
            name: name.to_string(),
            kind: "Textures source".to_string(),
            content: "files".to_string(),
            kept: Kept::Source,
            path: PathBuf::from(format!("sources/{name}.textures.src")),
            bytes: None,
            seen: Seen::NotCooked,
            id: Some(id.to_string()),
            facts: Vec::new(),
            output: None,
            textures: Vec::new(),
            target: None,
            source_kind: Some("textures".to_string()),
            from_game: false,
            origin: Some(origin.to_string()),
        }
    }

    fn scan_of(rows: Vec<AssetRow>) -> Scan {
        Scan {
            project: PathBuf::from("p.openrage-proj"),
            rows,
            plans: Vec::new(),
            links: Vec::new(),
            locks: Vec::new(),
            packs: Vec::new(),
            said: Vec::new(),
            peds: Vec::new(),
            weapons: Vec::new(),
            maps: Vec::new(),
            particles: Vec::new(),
            truncated: false,
        }
    }

    #[test]
    fn a_missing_file_is_named_by_its_place_in_the_project_and_not_by_where_the_project_lives() {
        // Paths only: nothing is made on the disk.
        let base = std::env::current_dir().unwrap().join("gadgets");
        let project = base.join("gadgets.openrage-proj");
        let mut missing = a_source("gone", "id-gone", "o");
        missing.name = "gone.ydd".to_string();
        missing.kept = Kept::File;
        missing.seen = Seen::Missing;
        missing.path = base.join("sources").join("gone.ydd");
        let mut scan = scan_of(vec![missing]);
        scan.project = project.clone();
        let found = collect(&scan, None, &HashSet::new(), &[], &Builds::default());
        let said: Vec<&str> = found.iter().map(|f| f.text.as_str()).collect();
        assert_eq!(said, ["gone.ydd is missing: sources/gone.ydd"]);

        // A file the project only references is somewhere else, and that place is what a person needs.
        let mut elsewhere = a_source("far", "id-far", "o");
        elsewhere.name = "far.ydd".to_string();
        elsewhere.kept = Kept::File;
        elsewhere.seen = Seen::Missing;
        elsewhere.path = base.parent().unwrap().join("other").join("far.ydd");
        let there = elsewhere.path.display().to_string();
        let mut scan = scan_of(vec![elsewhere]);
        scan.project = project;
        let found = collect(&scan, None, &HashSet::new(), &[], &Builds::default());
        assert_eq!(found[0].text, format!("far.ydd is missing: {there}"));
    }

    fn errors_of(rows: Vec<AssetRow>) -> Vec<Finding> {
        collect(
            &scan_of(rows),
            None,
            &HashSet::new(),
            &[],
            &Builds::default(),
        )
    }

    /// The folder each finding offers a new id to.
    fn offered_a_new_id(found: &[Finding]) -> Vec<PathBuf> {
        let mut folders: Vec<PathBuf> = found
            .iter()
            .flat_map(|f| &f.fixes)
            .map(|fix| match fix {
                Fix::Reid(folder) => folder.clone(),
                other => panic!("only a new id is offered here: {other:?}"),
            })
            .collect();
        folders.sort();
        folders
    }

    #[test]
    fn two_folders_with_one_id_each_offer_a_new_id_whichever_the_table_lists_first() {
        // ⚠ A hand copy keeps the manifest's name, so the table sorts the pair by folder: `a copy` lists before `a`,
        // and `a_copy` after it. Nothing in either folder says which is the copy, so neither is called one.
        for (first, second) in [("a copy", "a"), ("a", "a_copy")] {
            let found = errors_of(vec![
                a_source(first, "one-id", "Imported|true|x.ytd|aa"),
                a_source(second, "one-id", "Imported|true|x.ytd|aa"),
                a_source("d", "another-id", "Authored|false||"),
            ]);

            assert_eq!(found.len(), 2, "one error for each holder: {found:?}");
            assert!(found.iter().all(|f| f.severity == Severity::Error));
            let mut holders = vec![
                PathBuf::from(format!("sources/{first}.textures.src")),
                PathBuf::from(format!("sources/{second}.textures.src")),
            ];
            holders.sort();
            assert_eq!(
                offered_a_new_id(&found),
                holders,
                "each folder can be given the new id"
            );
            for finding in &found {
                assert_eq!(finding.fixes.len(), 1);
                assert_eq!(finding.fixes[0].label(), "New id");
                assert!(
                    finding.text.contains(&format!("{first}.textures.src"))
                        && finding.text.contains(&format!("{second}.textures.src")),
                    "the message names both folders: {}",
                    finding.text
                );
                assert!(
                    finding.text.contains("same file") && !finding.text.contains("copy of"),
                    "the same origin says one file and never which folder is the copy: {}",
                    finding.text
                );
            }
        }
    }

    #[test]
    fn folders_that_share_an_id_from_another_file_say_so_and_each_holder_is_a_row() {
        let found = errors_of(vec![
            a_source("a", "one-id", "Imported|true|x.ytd|aa"),
            a_source("b", "one-id", "Imported|true|x.ytd|aa"),
            a_source("c", "one-id", "Authored|false||"),
            a_source("d", "another-id", "Authored|false||"),
        ]);

        assert_eq!(found.len(), 3, "{found:?}");
        assert_eq!(
            offered_a_new_id(&found),
            [
                PathBuf::from("sources/a.textures.src"),
                PathBuf::from("sources/b.textures.src"),
                PathBuf::from("sources/c.textures.src"),
            ]
        );
        for finding in &found {
            for folder in ["a.textures.src", "b.textures.src", "c.textures.src"] {
                assert!(finding.text.contains(folder), "{}", finding.text);
            }
            assert!(
                finding.text.contains("do not come from one file")
                    && !finding.text.contains("copy"),
                "another origin is no copy: {}",
                finding.text
            );
        }
    }

    #[test]
    fn a_sentence_does_not_begin_with_the_name_its_row_already_says() {
        assert_eq!(
            without_subject("my cap has no texture", "my cap"),
            "Has no texture"
        );
        // A longer name that begins with it, a sentence about something else, and the name alone are left as they are.
        assert_eq!(
            without_subject("my cape has no texture", "my cap"),
            "my cape has no texture"
        );
        assert_eq!(
            without_subject("the shop id X is written twice", "my cap"),
            "the shop id X is written twice"
        );
        assert_eq!(without_subject("my cap", "my cap"), "my cap");
    }

    #[test]
    fn a_sentence_that_begins_with_the_possessive_of_the_name_loses_it_as_well() {
        assert_eq!(
            without_subject("my cap's model has no low level of detail", "my cap"),
            "Model has no low level of detail"
        );
        assert_eq!(
            without_subject("my cap's alternate model 2 draws a lot", "my cap"),
            "Alternate model 2 draws a lot"
        );
        // Another name that begins the same way is another subject.
        assert_eq!(
            without_subject("my cape's model has none", "my cap"),
            "my cape's model has none"
        );
    }

    /// A pack of one garment called `label` whose model and texture dictionary are counted to break every rule that reads a count, as a scan would have read them.
    fn a_pack_that_breaks_every_counted_rule(label: &str) -> Pack {
        use or_wardrobe::measure::{Drawable, Lod, Measure, Texture};
        use or_wardrobe::{Component, Fact, Facts, Garment, Ped, Slot, Variation, Wardrobe};

        let counted = |facts: &mut Facts, path: &str, measure: &Measure| {
            let measured = serde_json::to_value(measure).expect("a count is JSON");
            facts.insert(
                path,
                Fact::File {
                    bytes: 10,
                    digest: Some(path.into()),
                    read: None,
                    measured: Some(measured),
                },
            );
        };
        let texture = |name: &str, side: u32, mips: u32, format: &str| Texture {
            name: name.into(),
            width: side,
            height: side,
            mips,
            format: format.into(),
        };
        let level = |vertices: u64, triangles: u64| {
            Some(Lod {
                vertices,
                triangles,
            })
        };
        let huge = 1 << 40;

        let mut wardrobe = Wardrobe::new("pack");
        let mut garment = Garment::new();
        garment.label = Some(label.to_string());
        garment.ped = Some(Ped::Male);
        garment.slot = Some(Slot::Component(Component::Jbib));
        garment.model = Some("a.ydd".into());
        garment.variations.push(Variation {
            texture: Some("t0.ytd".into()),
            ..Variation::default()
        });
        wardrobe.garments.push(garment);

        let mut facts = Facts::default();
        let dictionary = Measure {
            streaming: huge,
            detail: true,
            textures: vec![
                texture("big", 16384, 1, "DXT1"),
                texture("odd", 1000, 10, "DXT1"),
                texture("raw", 1024, 11, "A8R8G8B8"),
                texture("seven", 1024, 11, "BC7"),
            ],
            ..Measure::default()
        };
        counted(&mut facts, "t0.ytd", &dictionary);
        // One drawable with no low level and a heavy medium one, and one with a heavy low level, too many triangles, a shader with no diffuse and a texture nothing uses.
        let model = Measure {
            detail: true,
            drawables: vec![
                Drawable {
                    name: "short".into(),
                    lods: [level(3000, 5000), level(2900, 4800), None, None],
                    shaders: 1,
                    ..Drawable::default()
                },
                Drawable {
                    name: "heavy".into(),
                    lods: [level(3000, huge), level(100, 100), level(huge, 100), None],
                    shaders: 1,
                    without_diffuse: vec![0],
                    unused_embedded: vec!["left_in".into()],
                    missing_parameters: vec![or_wardrobe::measure::Missing {
                        shader: 0,
                        names: vec!["bumpiness".into()],
                    }],
                },
            ],
            ..Measure::default()
        };
        counted(&mut facts, "a.ydd", &model);

        let allocation = or_wardrobe::allocate(&wardrobe);
        let findings = or_wardrobe::check(&wardrobe, &allocation, &facts);
        Pack {
            module: "clothing".into(),
            folder: PathBuf::from("clothing"),
            linked: false,
            read: Ok(super::super::wardrobe::PackRead {
                wardrobe,
                allocation,
                findings,
                drawables: 1,
                textures: 1,
                counts: Vec::new(),
                sizes: Default::default(),
                dependencies: Vec::new(),
            }),
            last_good: None,
        }
    }

    #[test]
    fn no_rule_that_reads_a_count_begins_its_sentence_with_the_name_the_row_shows() {
        let label = "Odd sized top";
        let mut scan = scan_of(Vec::new());
        scan.packs = vec![a_pack_that_breaks_every_counted_rule(label)];

        let found = collect(&scan, None, &HashSet::new(), &[], &Builds::default());

        let codes: std::collections::BTreeSet<&str> = found.iter().filter_map(|f| f.code).collect();
        // `lod-missing` is a note, which no row lists.
        let counted = [
            "texture-size",
            "texture-not-power-of-two",
            "texture-mips",
            "texture-format",
            "texture-bc7-legacy",
            "dictionary-size",
            "lod-medium-heavy",
            "lod-low-heavy",
            "polycount-high",
            "diffuse-missing",
            "embedded-unused",
            "material-parameters",
        ];
        for code in counted {
            assert!(codes.contains(code), "{code} was not broken: {codes:?}");
        }
        for finding in &found {
            assert!(
                matches!(&finding.place, Place::Garment { label: shown, .. } if shown == label),
                "{finding:?}"
            );
            assert!(
                !finding.text.contains(label),
                "the row says {label}, so its sentence does not: {}",
                finding.text
            );
        }
    }

    #[test]
    fn a_material_that_lacks_constants_is_offered_the_completion_beside_its_finding_in_a_pack_this_project_writes(
    ) {
        let mut scan = scan_of(Vec::new());
        scan.packs = vec![a_pack_that_breaks_every_counted_rule("Odd sized top")];
        let found = collect(&scan, None, &HashSet::new(), &[], &Builds::default());
        let lacking = found
            .iter()
            .find(|f| f.code == Some("material-parameters"))
            .expect("the finding is listed");
        assert!(
            lacking
                .fixes
                .iter()
                .any(|fix| matches!(fix, Fix::Materials { .. }) && fix.label() == "Complete"),
            "{:?}",
            lacking.fixes
        );
    }

    #[test]
    fn a_model_the_reader_refuses_is_offered_the_repair_beside_its_finding_in_a_pack_this_project_writes(
    ) {
        use or_wardrobe::{Component, Fact, Facts, Garment, Ped, Slot, Wardrobe};

        let mut wardrobe = Wardrobe::new("pack");
        let mut garment = Garment::new();
        garment.label = Some("Sealed top".into());
        garment.ped = Some(Ped::Male);
        garment.slot = Some(Slot::Component(Component::Jbib));
        garment.model = Some("a.ydd".into());
        let id = garment.id.clone();
        wardrobe.garments.push(garment);
        let mut facts = Facts::default();
        facts.insert(
            "a.ydd",
            Fact::File {
                bytes: 10,
                digest: Some("a".into()),
                read: Some(Err("a protected asset".into())),
                measured: None,
            },
        );
        let allocation = or_wardrobe::allocate(&wardrobe);
        let findings = or_wardrobe::check(&wardrobe, &allocation, &facts);
        let pack = |linked| Pack {
            module: "clothing".into(),
            folder: PathBuf::from("clothing"),
            linked,
            read: Ok(super::super::wardrobe::PackRead {
                wardrobe: wardrobe.clone(),
                allocation: allocation.clone(),
                findings: findings.clone(),
                drawables: 1,
                textures: 0,
                counts: Vec::new(),
                sizes: Default::default(),
                dependencies: Vec::new(),
            }),
            last_good: None,
        };
        let repairs = |linked| {
            let mut scan = scan_of(Vec::new());
            scan.packs = vec![pack(linked)];
            let found = collect(&scan, None, &HashSet::new(), &[], &Builds::default());
            let unreadable = found
                .into_iter()
                .find(|f| f.code == Some("model-unreadable"))
                .expect("the reader's refusal is a finding");
            unreadable
                .fixes
                .into_iter()
                .filter(|fix| matches!(fix, Fix::Repair { garment, .. } if *garment == id))
                .count()
        };
        assert_eq!(repairs(false), 1);
        assert_eq!(
            repairs(true),
            0,
            "a linked pack is another project's to repair"
        );
    }

    #[test]
    fn sources_with_ids_of_their_own_are_no_problem() {
        let scan = scan_of(vec![
            a_source("a", "1", "Authored|false||"),
            a_source("b", "2", "Authored|false||"),
        ]);
        assert!(collect(&scan, None, &HashSet::new(), &[], &Builds::default()).is_empty());
    }

    /// What a validator said of the pack in `pack`: a message that names a count and no pack, so two packs say the same, and the fix that writes that pack's file.
    fn said_of(pack: &str, level: Level, output: &str) -> Said {
        Said {
            plugin: "labeller".into(),
            name: "Labeller".into(),
            output: output.into(),
            resource: "mypack".into(),
            level,
            text: "plugin.labeller.unlabelled: 1 garment(s) have no label".into(),
            fix: Some(or_plugin::SuggestedFix {
                label: "Label them".into(),
                files: or_plugin::Transaction {
                    files: vec![or_plugin::FileWrite {
                        path: format!("{pack}/wardrobe.json"),
                        bytes: b"{}".to_vec(),
                    }],
                },
            }),
        }
    }

    /// The file the plugin's fix of each row writes, in the order of the rows.
    fn written_by(found: &[Finding]) -> Vec<String> {
        found
            .iter()
            .map(|finding| {
                let written: Vec<&str> = (finding.fixes.iter())
                    .filter_map(|fix| match fix {
                        Fix::Plugin { fix, .. } => Some(fix.files.files[0].path.as_str()),
                        _ => None,
                    })
                    .collect();
                assert_eq!(written.len(), 1, "{finding:?}");
                written[0].to_string()
            })
            .collect()
    }

    #[test]
    fn two_warnings_with_one_text_each_offer_the_fix_of_their_own_finding() {
        let mut scan = scan_of(Vec::new());
        scan.said = vec![
            said_of("male", Level::Warning, "fivem"),
            said_of("female", Level::Warning, "fivem"),
        ];

        let found = collect(&scan, None, &HashSet::new(), &[], &Builds::default());

        assert_eq!(
            written_by(&found),
            ["male/wardrobe.json", "female/wardrobe.json"]
        );
    }

    #[test]
    fn two_errors_with_one_text_each_offer_the_fix_of_their_own_finding_in_their_own_plan() {
        use crate::state::workbench::scan::{Planned, Resource};

        let planned = |output: &str, problems: &[String]| {
            let container = or_project::Container {
                modules: vec![or_project::Module::files("files")],
                outputs: vec![or_project::Output::fivem_resource(
                    output,
                    "mypack",
                    &["files"],
                )],
            };
            let mut plan = or_project::plan_with(
                &container,
                output,
                std::path::Path::new("p"),
                &[or_project::Contribution::of("files")],
            )
            .expect("the output is declared");
            plan.problems = problems.to_vec();
            Planned {
                output: output.into(),
                resources: vec![Resource { plan, since: None }],
                sizes: Vec::new(),
            }
        };
        let text = said_of("", Level::Error, "").text;
        let mut scan = scan_of(Vec::new());
        scan.plans = vec![
            planned("one", &[text.clone(), text.clone()]),
            planned("two", std::slice::from_ref(&text)),
        ];
        scan.said = vec![
            said_of("male", Level::Error, "one"),
            said_of("female", Level::Error, "one"),
            said_of("props", Level::Error, "two"),
        ];

        let found = collect(&scan, None, &HashSet::new(), &[], &Builds::default());

        assert_eq!(
            written_by(&found),
            [
                "male/wardrobe.json",
                "female/wardrobe.json",
                "props/wardrobe.json"
            ]
        );
    }
}
