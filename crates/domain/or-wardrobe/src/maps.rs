//! Normal and specular maps for the textures of a pack, made from the diffuse: new pictures in a copy of the source, as one step of Undo.
//!
//! A source is never changed in place. One that gets maps is copied beside itself under a free name, with an id of its own, the
//! maps are written into the copy, and the garments in scope name the copy; the source it was copied from is untouched, so Undo
//! takes the copy out and the pack names the old source again. [`plan`] says what would be made and copies nothing.
//!
//! What a map is, and why it is an estimate, is [`or_source::maps`]'s.

use std::path::Path;

use or_source::maps::{self, Generated, Options};
use serde::Serialize;

use crate::edit::{Copying, Error, Revision, Watch};
use crate::model::Wardrobe;
use crate::textures::{finish, join, over_sources, who, Left};

/// A source that was copied and given maps, and the garments that name the copy now.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Made {
    pub garments: Vec<String>,
    pub from: String,
    /// The copy, none for a plan.
    pub into: Option<String>,
    pub generated: Vec<Generated>,
}

/// What a request made of a pack, or would make.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Report {
    pub made: Vec<Made>,
    pub left: Vec<Left>,
}

impl Report {
    /// How many maps.
    pub fn maps(&self) -> usize {
        self.made
            .iter()
            .flat_map(|m| &m.generated)
            .map(|g| g.maps.len())
            .sum()
    }

    /// The report as lines a person reads: each source with the maps it gets, then each texture left with its reason.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for one in &self.made {
            lines.push(match &one.into {
                Some(into) => format!("{} is now {into} for {}", one.from, one.garments.join(", ")),
                None => format!(
                    "{} would get maps for {}",
                    one.from,
                    one.garments.join(", ")
                ),
            });
            for g in &one.generated {
                for map in &g.maps {
                    lines.push(format!(
                        "  {} {} from {}",
                        map.kind.label(),
                        map.texture,
                        g.diffuse
                    ));
                }
            }
        }
        for left in &self.left {
            let which = left.texture.as_ref().map_or_else(
                || left.file.clone(),
                |texture| format!("{texture} of {}", left.file),
            );
            lines.push(format!("{which} was left as it is: {}", left.why));
        }
        lines
    }
}

fn run(
    wardrobe: &mut Wardrobe,
    copying: &mut Copying,
    ids: Option<&[String]>,
    options: &Options,
    dry: bool,
    watch: &Watch,
) -> Result<Report, Error> {
    if !(options.normal || options.specular) {
        return Err(Error::Refused(
            "ask for a normal map, a specular map or both".into(),
        ));
    }
    if !(options.strength.is_finite() && options.strength > 0.0) {
        return Err(Error::Refused(format!(
            "a strength is above 0, and {} is not",
            options.strength
        )));
    }
    let folder = copying.folder.to_path_buf();
    let mut report = Report::default();
    let mut made = Vec::new();
    let mut left = Vec::new();
    over_sources(
        wardrobe,
        copying,
        ids,
        watch,
        &mut left,
        |wardrobe, copying, rel, left| {
            let path = folder.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
            let one = |why: String| Left {
                file: rel.to_string(),
                texture: None,
                why,
            };
            let plan = match maps::plan(&path, options) {
                Ok(plan) => plan,
                Err(problems) => {
                    left.push(one(join(&problems)));
                    return Ok(None);
                }
            };
            left.extend(plan.left.iter().map(|l| Left {
                file: rel.to_string(),
                texture: Some(l.texture.clone()),
                why: l.why.clone(),
            }));
            if plan.generated.is_empty() {
                return Ok(None);
            }
            let garments = who(wardrobe, ids, rel);
            if dry {
                made.push(Made {
                    garments,
                    from: rel.to_string(),
                    into: None,
                    generated: plan.generated,
                });
                return Ok(None);
            }
            let mark = copying.mark();
            let into = match copying.duplicate_source(rel) {
                Ok(into) => into,
                Err(e) => {
                    copying.rollback_to(mark);
                    left.push(one(format!("it could not be copied: {e}")));
                    return Ok(None);
                }
            };
            let stop = || watch.cancel.is_set();
            match maps::apply(&folder.join(&into), options, &stop) {
                Ok(done) => {
                    // The pictures are new files of the copy, so Undo is told of them as it is of the copy's own.
                    for map in done.generated.iter().flat_map(|g| &g.maps) {
                        copying.copied.push(format!(
                            "{into}/{}/{}",
                            or_source::textures::DIR,
                            map.file
                        ));
                    }
                    made.push(Made {
                        garments,
                        from: rel.to_string(),
                        into: Some(into.clone()),
                        generated: done.generated,
                    });
                    Ok(Some(into))
                }
                Err(problems) => {
                    copying.rollback_to(mark);
                    watch.check()?;
                    left.push(one(join(&problems)));
                    Ok(None)
                }
            }
        },
    )?;
    report.made = made;
    report.left = left;
    Ok(report)
}

/// The maps `options` would make for the garments `ids` (every garment where none) of the pack in `folder`; nothing is copied or written.
pub fn plan(folder: &Path, ids: Option<&[String]>, options: &Options) -> Result<Report, Error> {
    let mut wardrobe = Wardrobe::read(folder)?;
    let mut copying = Copying::new(folder);
    run(
        &mut wardrobe,
        &mut copying,
        ids,
        options,
        true,
        &Watch::default(),
    )
}

/// Make the maps for the garments `ids` (every garment where none) of the pack in `folder`, with the Undo step that takes them back; none where there was nothing to make.
///
/// ⚠ Stopped by `watch`, the pack and its folder are as they were.
pub fn generate(
    folder: &Path,
    ids: Option<&[String]>,
    options: &Options,
    watch: &Watch,
) -> Result<(Report, Option<Revision>), Error> {
    let before = Wardrobe::read(folder)?;
    let mut wardrobe = before.clone();
    let mut copying = Copying::new(folder);
    let report = match run(&mut wardrobe, &mut copying, ids, options, false, watch) {
        Ok(report) => report,
        Err(e) => {
            copying.undo();
            return Err(e);
        }
    };
    let revision = finish(folder, before, &wardrobe, copying)?;
    Ok((report, revision))
}
