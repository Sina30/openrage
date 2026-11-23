//! The rules that hold what a file states of itself to a limit: the textures of a garment and its models.
//!
//! A file nobody counted, or that the owned readers do not open, is not judged: a rule that needs a count is left out
//! for it, and the reader's own refusal is a finding of its own (`model-unreadable`). A garment with many textures at
//! fault is one finding that counts them and names the first, so a pack of thousands is a list a person can read.

use super::Report;
use crate::facts::Facts;
use crate::measure::{Drawable, Lod, Measure, Texture};
use crate::model::Garment;
use crate::names::Skin;
use crate::settings::Target;

/// The words a skin-tone texture's name ends with: the freemode peds' skin tones.
const SKIN_TONES: &[&str] = &["whi", "bla", "chi", "lat", "pak", "ara"];

/// Whether a texture's name ends with a skin tone, as `jbib_diff_000_a_whi` does.
fn is_skin_tone(name: &str) -> bool {
    name.rsplit('_')
        .next()
        .is_some_and(|word| SKIN_TONES.iter().any(|t| word.eq_ignore_ascii_case(t)))
}

/// The formats the clothing textures of the game's own peds are stored in.
const CLOTHING_FORMATS: &[&str] = &["DXT1", "DXT3", "DXT5", "DXN"];

/// The textures of the garment `g` and its models, held to the limits of the rules that are on.
pub(super) fn garment(r: &mut Report, g: &Garment, who: &str, facts: &Facts) {
    let dictionaries: Vec<(usize, Measure)> = (g.variations.iter().enumerate())
        .filter_map(|(i, v)| Some((i, Measure::of_fact(facts.get(v.texture.as_deref()?))?)))
        .filter(|(_, m)| m.detail)
        .collect();
    textures(r, g, who, &dictionaries);
    let models: Vec<(String, Measure)> = (g.model.iter().map(|m| ("model".to_string(), m)))
        .chain(
            (g.alternates.iter().enumerate())
                .map(|(i, m)| (format!("alternate model {}", i + 1), m)),
        )
        .filter_map(|(role, path)| Some((role, Measure::of_fact(facts.get(path))?)))
        .filter(|(_, m)| m.detail)
        .collect();
    models_of(r, g, who, &models);
}

/// `n` of what `one` names, `1 texture` or `3 textures`.
fn counted(n: usize, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

/// A size in bytes as megabytes, to a tenth.
fn megabytes(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / (1024.0 * 1024.0))
}

/// The textures of `files` that `fails`: how many, the first, and the index of the garment's texture it is in.
fn failing(
    files: &[(usize, Measure)],
    fails: impl Fn(&Texture) -> bool,
) -> Option<(usize, &Texture, usize)> {
    let mut found = (files.iter())
        .flat_map(|(i, m)| m.textures.iter().map(move |t| (*i, t)))
        .filter(|(_, t)| fails(t));
    let (at, first) = found.next()?;
    Some((at, first, 1 + found.count()))
}

fn textures(r: &mut Report, g: &Garment, who: &str, files: &[(usize, Measure)]) {
    let side = r.limit("texture-size");
    if let Some((at, first, n)) = failing(files, |t| u64::from(t.width.max(t.height)) > side) {
        r.add_about(
            "texture-size",
            Some(g),
            Some(at),
            format!(
                "{who} has {} over {side} pixels on a side, the first {} at {} by {}",
                counted(n, "texture"),
                first.name,
                first.width,
                first.height
            ),
        );
    }
    if g.skin == Skin::Race {
        let tone = r.limit("texture-skin-tone-size");
        let off = |t: &Texture| {
            is_skin_tone(&t.name) && (u64::from(t.width) != tone || u64::from(t.height) != tone)
        };
        if let Some((at, first, n)) = failing(files, off) {
            r.add_about(
                "texture-skin-tone-size",
                Some(g),
                Some(at),
                format!(
                    "{who} has {} that is a skin tone and not {tone} by {tone}, the first {} at {} by {}",
                    counted(n, "texture"),
                    first.name,
                    first.width,
                    first.height
                ),
            );
        }
    }
    let uneven = |t: &Texture| !t.width.is_power_of_two() || !t.height.is_power_of_two();
    if let Some((at, first, n)) = failing(files, uneven) {
        r.add_about(
            "texture-not-power-of-two",
            Some(g),
            Some(at),
            format!(
                "{who} has {} whose width or height is not a power of two, the first {} at {} by {}",
                counted(n, "texture"),
                first.name,
                first.width,
                first.height
            ),
        );
    }
    if let Some((at, first, n)) = failing(files, |t| t.mips < 2 && t.width.max(t.height) > 4) {
        r.add_about(
            "texture-mips",
            Some(g),
            Some(at),
            format!(
                "{who} has {} with no smaller sizes (mipmaps), the first {} at {} by {}",
                counted(n, "texture"),
                first.name,
                first.width,
                first.height
            ),
        );
    }
    // ⚠ BC7 has a rule of its own, since where it is wrong depends on the edition the pack is built for.
    let other = |t: &Texture| t.format != "BC7" && !CLOTHING_FORMATS.contains(&t.format.as_str());
    if let Some((at, first, n)) = failing(files, other) {
        r.add_about(
            "texture-format",
            Some(g),
            Some(at),
            format!(
                "{who} has {} stored in a format the game's own clothing is not, the first {} as {}",
                counted(n, "texture"),
                first.name,
                first.format
            ),
        );
    }
    if r.wardrobe.rules.holds(Target::Legacy) {
        if let Some((at, first, n)) = failing(files, |t| t.format == "BC7") {
            r.add_about(
                "texture-bc7-legacy",
                Some(g),
                Some(at),
                format!(
                    "{who} has {} stored as BC7, the first {}, which a DirectX 10 card cannot read",
                    counted(n, "texture"),
                    first.name
                ),
            );
        }
    }
    let heaviest = r.limit("dictionary-size");
    let over: Vec<&(usize, Measure)> = files
        .iter()
        .filter(|(_, m)| m.streaming > heaviest)
        .collect();
    if let Some((at, first)) = over.first() {
        r.add_about(
            "dictionary-size",
            Some(g),
            Some(*at),
            format!(
                "{who} has {} of {} MB, over the {} MB a dictionary is held to",
                match over.len() {
                    1 => "a texture dictionary".to_string(),
                    n => format!("{n} texture dictionaries, the first"),
                },
                megabytes(first.streaming),
                megabytes(heaviest)
            ),
        );
    }
}

/// The drawables of `models`, each with the role of the model file it is in.
fn drawables(models: &[(String, Measure)]) -> impl Iterator<Item = (&str, &Drawable)> {
    models
        .iter()
        .flat_map(|(role, m)| m.drawables.iter().map(move |d| (role.as_str(), d)))
}

fn models_of(r: &mut Report, g: &Garment, who: &str, models: &[(String, Measure)]) {
    /// The first drawable of `models` that `fails`, with the role of its file and how many fail.
    fn first(
        models: &[(String, Measure)],
        fails: impl Fn(&Drawable) -> bool,
    ) -> Option<(&str, &Drawable, usize)> {
        let mut found = drawables(models).filter(|(_, d)| fails(d));
        let (role, d) = found.next()?;
        Some((role, d, 1 + found.count()))
    }
    let say = |r: &mut Report, code: &'static str, role: &str, n: usize, what: String| {
        let others = match n {
            1 => String::new(),
            n => format!(", and {} more", n - 1),
        };
        r.add_about(
            code,
            Some(g),
            None,
            format!("{who}'s {role} {what}{others}"),
        );
    };
    let lod = |d: &Drawable, level: usize| {
        d.lods[level].unwrap_or(Lod {
            vertices: 0,
            triangles: 0,
        })
    };

    if let Some((role, _, n)) = first(models, |d| {
        d.lods[0].is_some() && (d.lods[1].is_none() || d.lods[2].is_none())
    }) {
        let what = "has no medium or no low level of detail, so it is drawn at its full cost wherever it is seen from";
        say(r, "lod-missing", role, n, what.into());
    }
    let share = r.limit("lod-medium-heavy");
    if let Some((role, d, n)) = first(models, |d| match (d.lods[0], d.lods[1]) {
        (Some(high), Some(medium)) => medium.vertices * 100 > high.vertices * share,
        _ => false,
    }) {
        let what = format!(
            "has a medium level of detail of {} vertices, which is more than {share} percent of the {} of its high level",
            lod(d, 1).vertices,
            lod(d, 0).vertices
        );
        say(r, "lod-medium-heavy", role, n, what);
    }
    let low = r.limit("lod-low-heavy");
    if let Some((role, d, n)) = first(models, |d| d.lods[2].is_some_and(|l| l.vertices > low)) {
        let what = format!(
            "has a low level of detail of {} vertices, which is more than the {low} it is held to",
            lod(d, 2).vertices
        );
        say(r, "lod-low-heavy", role, n, what);
    }
    let ceiling = r.limit("polycount-high");
    if let Some((role, d, n)) = first(models, |d| d.lods[0].is_some_and(|l| l.triangles > ceiling))
    {
        let what = format!(
            "draws {} triangles at its high level of detail, over the {ceiling} it is held to",
            lod(d, 0).triangles
        );
        say(r, "polycount-high", role, n, what);
    }
    if let Some((role, d, n)) = first(models, |d| !d.without_diffuse.is_empty()) {
        let what = format!(
            "draws with {} that bind no diffuse texture, so the garment shows none",
            counted(d.without_diffuse.len(), "shader")
        );
        say(r, "diffuse-missing", role, n, what);
    }
    if let Some((role, d, n)) = first(models, |d| !d.missing_parameters.is_empty()) {
        let first = &d.missing_parameters[0];
        let what = format!(
            "draws with {} that lack constants their shader declares, the first {} of shader {}",
            counted(d.missing_parameters.len(), "material"),
            first.names[0],
            first.shader
        );
        say(r, "material-parameters", role, n, what);
    }
    if let Some((role, d, n)) = first(models, |d| !d.unused_embedded.is_empty()) {
        let what = format!(
            "carries {}, the first {}, that none of its shaders uses, which adds its size to every streaming of the model",
            counted(d.unused_embedded.len(), "texture"),
            d.unused_embedded[0]
        );
        say(r, "embedded-unused", role, n, what);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc::allocate;
    use crate::facts::Fact;
    use crate::model::{Variation, Wardrobe};
    use crate::names::{Component, Ped, Skin, Slot};
    use crate::rules::{check, Finding, Severity};
    use crate::settings::{Level, Setting};

    const MEBIBYTE: u64 = 1024 * 1024;

    /// Where the rule `code` holds a value to by its own row, which a test of its edge takes from the row and not from a number it copies.
    fn limit(code: &str) -> u64 {
        crate::rules::rule_of(code)
            .and_then(|rule| rule.threshold)
            .map(|t| t.default)
            .expect("a rule with a limit")
    }

    /// The mips of a whole chain for a texture `side` pixels on a side.
    fn whole(side: u32) -> u32 {
        32 - side.leading_zeros()
    }

    fn texture(name: &str, side: u32, mips: u32, format: &str) -> Texture {
        Texture {
            name: name.into(),
            width: side,
            height: side,
            mips,
            format: format.into(),
        }
    }

    /// A texture that breaks no rule: a square of 1024, every mip, compressed the way the game's clothing is.
    fn sound() -> Texture {
        texture("jbib_diff_000_a_uni", 1024, 11, "DXT1")
    }

    fn dictionary(textures: Vec<Texture>) -> Measure {
        Measure {
            streaming: MEBIBYTE,
            detail: true,
            enhanced: false,
            textures,
            drawables: Vec::new(),
        }
    }

    fn level(vertices: u64, triangles: u64) -> Option<Lod> {
        Some(Lod {
            vertices,
            triangles,
        })
    }

    /// A drawable that breaks no rule: four levels each lighter than the last, one shader that binds its diffuse, nothing carried.
    fn solid() -> Drawable {
        Drawable {
            name: "jbib_000_u".into(),
            lods: [
                level(3000, 5000),
                level(1200, 2000),
                level(300, 500),
                level(100, 150),
            ],
            shaders: 1,
            without_diffuse: Vec::new(),
            unused_embedded: Vec::new(),
            missing_parameters: Vec::new(),
        }
    }

    fn model(drawable: Drawable) -> Measure {
        Measure {
            streaming: MEBIBYTE,
            detail: true,
            enhanced: false,
            textures: Vec::new(),
            drawables: vec![drawable],
        }
    }

    fn counted(facts: &mut Facts, path: &str, measure: Option<&Measure>) {
        facts.insert(
            path,
            Fact::File {
                bytes: 10,
                digest: Some(path.into()),
                read: None,
                measured: measure.map(|m| serde_json::to_value(m).expect("a count is JSON")),
            },
        );
    }

    /// One garment of the jacket slot with a model and a texture dictionary for each of `dictionaries`.
    fn pack(model: &Measure, dictionaries: &[Measure]) -> (Wardrobe, Facts) {
        let mut w = Wardrobe::new("pack");
        let mut g = Garment::new();
        g.ped = Some(Ped::Male);
        g.slot = Some(Slot::Component(Component::Jbib));
        g.model = Some("a.ydd".into());
        let mut facts = Facts::default();
        counted(&mut facts, "a.ydd", Some(model));
        for (i, dictionary) in dictionaries.iter().enumerate() {
            let path = format!("t{i}.ytd");
            g.variations.push(Variation {
                texture: Some(path.clone()),
                ..Variation::default()
            });
            counted(&mut facts, &path, Some(dictionary));
        }
        w.garments.push(g);
        (w, facts)
    }

    fn found(w: &Wardrobe, facts: &Facts) -> Vec<Finding> {
        check(w, &allocate(w), facts)
    }

    fn codes(w: &Wardrobe, facts: &Facts) -> Vec<&'static str> {
        found(w, facts).iter().map(|f| f.code).collect()
    }

    #[test]
    fn a_garment_whose_files_are_counted_and_within_every_limit_has_nothing_wrong() {
        let (w, facts) = pack(&model(solid()), &[dictionary(vec![sound()])]);
        assert_eq!(codes(&w, &facts), Vec::<&str>::new());
    }

    #[test]
    fn each_texture_rule_finds_the_texture_built_to_break_it_and_only_that_rule() {
        let breaks: [(&str, Texture); 5] = [
            ("texture-size", texture("big", 4096, whole(4096), "DXT1")),
            ("texture-not-power-of-two", texture("odd", 1000, 10, "DXT1")),
            ("texture-mips", texture("flat", 1024, 1, "DXT1")),
            ("texture-format", texture("raw", 1024, 11, "A8R8G8B8")),
            ("texture-bc7-legacy", texture("seven", 1024, 11, "BC7")),
        ];
        for (code, bad) in breaks {
            let (w, facts) = pack(&model(solid()), &[dictionary(vec![bad])]);
            let said = found(&w, &facts);
            assert_eq!(
                said.iter().map(|f| f.code).collect::<Vec<_>>(),
                [code],
                "{code}: {said:?}"
            );
            assert_eq!(said[0].severity, Severity::Warning);
            assert_eq!(said[0].texture, Some(0), "it says which texture");
            assert!(
                !said[0].blocks(),
                "a limit the engine does not enforce is a warning"
            );
        }
    }

    #[test]
    fn a_skin_tone_texture_of_a_race_garment_is_held_to_the_size_the_skins_are() {
        let race = |textures: Vec<Texture>| {
            let (mut w, facts) = pack(&model(solid()), &[dictionary(textures)]);
            w.garments[0].skin = Skin::Race;
            (w, facts)
        };
        let side = limit("texture-skin-tone-size") as u32;
        let tone = |width: u32, height: u32| Texture {
            name: "jbib_diff_000_a_whi".into(),
            width,
            height,
            mips: whole(width.max(height)),
            format: "DXT1".into(),
        };

        let (w, facts) = race(vec![tone(side, side)]);
        assert_eq!(codes(&w, &facts), Vec::<&str>::new(), "the size is kept");
        for (width, height) in [(side / 2, side / 2), (side * 2, side * 2), (side, side / 2)] {
            let (w, facts) = race(vec![tone(width, height)]);
            let said = found(&w, &facts);
            assert_eq!(
                said.iter().map(|f| f.code).collect::<Vec<_>>(),
                ["texture-skin-tone-size"],
                "{width} by {height}: {said:?}"
            );
            assert_eq!(said[0].severity, Severity::Warning);
            assert!(
                said[0].message.contains("jbib_diff_000_a_whi")
                    && said[0].message.contains(&format!("{width} by {height}"))
                    && said[0].message.contains(&format!("{side} by {side}")),
                "{}",
                said[0].message
            );
        }

        // A texture that is not a skin tone is not held to it, and nor is a garment that fits every skin.
        let mut other = tone(side * 2, side * 2);
        other.name = "jbib_diff_000_a_uni".into();
        let (w, facts) = race(vec![other]);
        assert_eq!(codes(&w, &facts), Vec::<&str>::new());
        let (w, facts) = pack(
            &model(solid()),
            &[dictionary(vec![tone(side * 2, side * 2)])],
        );
        assert_eq!(codes(&w, &facts), Vec::<&str>::new());
    }

    #[test]
    fn a_limit_is_kept_by_the_value_on_it_and_broken_by_the_next() {
        let at = |side| {
            pack(
                &model(solid()),
                &[dictionary(vec![texture("t", side, whole(side), "DXT1")])],
            )
        };
        let side = limit("texture-size") as u32;
        let (w, facts) = at(side);
        assert_eq!(codes(&w, &facts), Vec::<&str>::new(), "the limit is kept");
        let (w, facts) = at(side * 2);
        assert_eq!(codes(&w, &facts), ["texture-size"]);

        let mut sized = dictionary(vec![sound()]);
        sized.streaming = limit("dictionary-size");
        let (w, facts) = pack(&model(solid()), &[sized.clone()]);
        assert_eq!(codes(&w, &facts), Vec::<&str>::new(), "the limit is kept");
        sized.streaming += 1;
        let (w, facts) = pack(&model(solid()), &[sized]);
        assert_eq!(codes(&w, &facts), ["dictionary-size"]);
    }

    #[test]
    fn bc7_is_held_to_the_edition_a_pack_is_built_for() {
        let seven = dictionary(vec![texture("seven", 1024, 11, "BC7")]);
        let (mut w, facts) = pack(&model(solid()), &[seven]);
        assert_eq!(
            codes(&w, &facts),
            ["texture-bc7-legacy"],
            "a pack built for none is built for both"
        );
        w.rules.target = Some(Target::Legacy);
        assert_eq!(codes(&w, &facts), ["texture-bc7-legacy"]);
        w.rules.target = Some(Target::Enhanced);
        assert_eq!(codes(&w, &facts), Vec::<&str>::new(), "Enhanced reads BC7");
    }

    #[test]
    fn bc7_and_another_format_in_one_garment_are_each_said_once_by_the_rule_that_is_its_own() {
        let both = dictionary(vec![
            texture("seven", 1024, 11, "BC7"),
            texture("raw", 1024, 11, "A8R8G8B8"),
        ]);
        let (w, facts) = pack(&model(solid()), &[both]);
        let said = found(&w, &facts);
        let by = |code: &str| {
            said.iter()
                .find(|f| f.code == code)
                .map(|f| f.message.clone())
        };
        assert_eq!(said.len(), 2, "{said:?}");
        assert!(by("texture-format")
            .is_some_and(|m| m.contains("1 texture") && m.contains("raw as A8R8G8B8")));
        assert!(by("texture-bc7-legacy")
            .is_some_and(|m| m.contains("1 texture") && m.contains("seven")));
    }

    #[test]
    fn each_model_rule_finds_the_model_built_to_break_it() {
        let mut no_medium = solid();
        no_medium.lods[1] = None;
        let mut no_low = solid();
        no_low.lods[2] = None;
        let mut heavy_medium = solid();
        let share = limit("lod-medium-heavy");
        heavy_medium.lods[1] = level(3000 * share / 100 + 1, 2500);
        let mut heavy_low = solid();
        heavy_low.lods[2] = level(limit("lod-low-heavy") + 1, 800);
        let mut dense = solid();
        dense.lods[0] = level(20_000, limit("polycount-high") + 1);
        let mut no_diffuse = solid();
        no_diffuse.without_diffuse = vec![0];
        let mut carries = solid();
        carries.unused_embedded = vec!["stray".into()];
        let mut lacks = solid();
        lacks.missing_parameters = vec![crate::measure::Missing {
            shader: 0,
            names: vec!["bumpiness".into()],
        }];
        let breaks = [
            ("lod-missing", no_medium),
            ("lod-missing", no_low),
            ("lod-medium-heavy", heavy_medium),
            ("lod-low-heavy", heavy_low),
            ("polycount-high", dense),
            ("diffuse-missing", no_diffuse),
            ("embedded-unused", carries),
            ("material-parameters", lacks),
        ];
        for (code, bad) in breaks {
            let (w, facts) = pack(&model(bad), &[dictionary(vec![sound()])]);
            let said = found(&w, &facts);
            // A model that breaks one rule may break another: 20,000 vertices high puts the medium level over its share.
            let own: Vec<_> = said.iter().filter(|f| f.code == code).collect();
            assert_eq!(own.len(), 1, "{code}: {said:?}");
            assert!(!own[0].blocks());
        }
        // The edge of each limit is kept: half the vertices, 500 vertices and the ceiling of triangles.
        let mut edge = solid();
        edge.lods[0] = level(3000, limit("polycount-high"));
        edge.lods[1] = level(3000 * share / 100, 2000);
        edge.lods[2] = level(limit("lod-low-heavy"), 600);
        let (w, facts) = pack(&model(edge), &[dictionary(vec![sound()])]);
        assert_eq!(codes(&w, &facts), Vec::<&str>::new());
    }

    #[test]
    fn a_garment_with_many_textures_at_fault_is_one_finding_that_counts_them() {
        let bad = |name: &str| texture(name, 4096, whole(4096), "DXT1");
        let (w, facts) = pack(
            &model(solid()),
            &[
                dictionary(vec![sound()]),
                dictionary(vec![bad("first"), bad("second")]),
                dictionary(vec![bad("third")]),
            ],
        );
        let said = found(&w, &facts);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].message.contains("3 textures") && said[0].message.contains("first"),
            "{}",
            said[0].message
        );
        assert_eq!(
            said[0].texture,
            Some(1),
            "the texture it names is the first at fault"
        );
    }

    #[test]
    fn a_file_nobody_counted_is_not_judged_whatever_it_is() {
        let huge = Measure {
            streaming: 1000 * MEBIBYTE,
            detail: false,
            ..dictionary(vec![texture("big", 8192, 1, "A8R8G8B8")])
        };
        let (w, mut facts) = pack(&model(solid()), &[huge]);
        assert_eq!(
            codes(&w, &facts),
            Vec::<&str>::new(),
            "the readers did not open it"
        );
        counted(&mut facts, "t0.ytd", None);
        assert_eq!(codes(&w, &facts), Vec::<&str>::new(), "no scan counted it");
    }

    #[test]
    fn a_rule_the_pack_switches_off_finds_nothing_and_one_it_raises_stops_a_build() {
        let big = dictionary(vec![texture("big", 4096, whole(4096), "DXT1")]);
        let (mut w, facts) = pack(&model(solid()), &[big]);
        assert_eq!(codes(&w, &facts), ["texture-size"]);

        w.rules.set("texture-size", |s| s.on = Some(false));
        assert_eq!(
            codes(&w, &facts),
            Vec::<&str>::new(),
            "a disabled rule is silent"
        );

        w.rules.set("texture-size", |s| s.on = Some(true));
        w.rules
            .set("texture-size", |s| s.severity = Some(Level::Error));
        let said = found(&w, &facts);
        assert_eq!(said.len(), 1);
        assert!(
            said[0].blocks(),
            "an error that is not waived stops a build"
        );

        w.rules.set("texture-size", |s| s.severity = None);
        w.rules.set("texture-size", |s| s.threshold = Some(8192));
        assert_eq!(
            codes(&w, &facts),
            Vec::<&str>::new(),
            "a limit moved up admits the texture"
        );
        w.rules.set("texture-size", |s| s.threshold = Some(512));
        let said = found(&w, &facts);
        assert!(
            said[0].message.contains("over 512 pixels"),
            "{}",
            said[0].message
        );
    }

    #[test]
    fn what_a_pack_sets_that_changes_nothing_is_said_and_changes_nothing() {
        let (mut w, facts) = pack(&model(solid()), &[dictionary(vec![sound()])]);
        let off = Setting {
            on: Some(false),
            ..Setting::default()
        };
        w.rules.settings.insert("no-such-rule".into(), off.clone());
        w.rules.settings.insert("model-missing".into(), off);
        w.rules.settings.insert(
            "duplicate-content".into(),
            Setting {
                threshold: Some(3),
                ..Setting::default()
            },
        );
        let said = found(&w, &facts);
        assert_eq!(
            said.iter().map(|f| f.code).collect::<Vec<_>>(),
            ["setting-unheeded", "setting-unheeded", "setting-unheeded"]
        );
        assert!(said
            .iter()
            .all(|f| f.severity == Severity::Warning && !f.blocks()));
        // An error no reason mends is still an error.
        let mut missing = facts.clone();
        missing.insert("a.ydd", Fact::Missing);
        assert!(codes(&w, &missing).contains(&"model-missing"));
    }
}
