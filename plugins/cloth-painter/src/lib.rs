//! The cloth painter: a plugin that paints the per-vertex values of a `.yld` in a panel and shows them
//! over the viewport, and writes the painted dictionary back as the open document.
//!
//! It extends and never hosts. It holds its model in this module's memory between frames, reads the
//! open file's bytes with `or-io` linked in, and hands bytes back to the host, which reads them again and
//! saves them in canonical form before it keeps them. Nothing here is a panel, a pick or a file of
//! the editor's: the host draws the widgets this describes.
//!
//! ⚠ Vertices are chosen in the panel, by region or by index, and never by clicking: the viewport
//! delivers a pick on a drawable and not on the cloth of a `.yld`.

mod cloth;

use std::cell::RefCell;

use cloth::{Dictionary, Paint, LODS};
use or_io::gta5::File;
use or_plugin_guest::{
    declare_setting, file_bytes, register_command, register_panel, set_overlay, setting_value, ui,
    write_file, Primitive, SettingKind, SettingValue, Status,
};

const PANEL: &str = "painter";
const COMMAND_WRITE: &str = "write";
const SETTING_SHOW: &str = "overlay";
const SETTING_SIZE: &str = "size";
const AXES: [&str; 3] = ["X", "Y", "Z"];
/// How many strokes the painter takes back.
const UNDO: usize = 20;
/// What the brush will set, typed as numbers: no range is known for them but what the staged cloths
/// hold (weights from 0 to 0.005, inflation scales of 0, pin radii from 0 to 1, a force of 0), and a
/// slider with a range of its own would forbid the values a cloth might need.
#[derive(Clone)]
struct Brush {
    weight: String,
    inflation: String,
    pin_radius: String,
    gradient_from: String,
    gradient_to: String,
    axis: u32,
    force: [String; 3],
    negate: bool,
}

/// `text` as a finite number, or the sentence that says which one is not.
fn number(text: &str, what: &str) -> Result<f32, String> {
    text.trim()
        .parse::<f32>()
        .ok()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("{what} is `{text}`, which is not a number"))
}


struct Model {
    /// The open file's bytes, as a hash, so a frame parses again only when they changed.
    signature: Option<u64>,
    dict: Option<Dictionary>,
    problem: String,
    entry: usize,
    lod: usize,
    selection: Vec<bool>,
    region_lo: [f32; 3],
    region_hi: [f32; 3],
    indices: String,
    brush: Brush,
    history: Vec<Dictionary>,
    status: String,
    /// The file the host holds is the one this wrote, so the next change of it is ours.
    wrote: bool,
    /// What the overlay shows is stale.
    dirty: bool,
    /// The settings the overlay was last drawn under.
    last: Option<(bool, u32)>,
}

impl Model {
    fn new() -> Self {
        Self {
            signature: None,
            dict: None,
            problem: "No document is open.".to_string(),
            entry: 0,
            lod: 0,
            selection: Vec::new(),
            region_lo: [0.0; 3],
            region_hi: [1.0; 3],
            indices: String::new(),
            brush: Brush {
                weight: "0".into(),
                inflation: "0".into(),
                pin_radius: "0".into(),
                gradient_from: "0".into(),
                gradient_to: "0.005".into(),
                axis: 2,
                force: ["0".into(), "0".into(), "0".into()],
                negate: false,
            },
            history: Vec::new(),
            status: String::new(),
            wrote: false,
            dirty: true,
            last: None,
        }
    }

    /// Read the open file again when its bytes are not the ones the model was made from.
    fn sync(&mut self) {
        let bytes = match file_bytes() {
            Ok(bytes) => bytes,
            Err(Status::Refused) => return self.close("No document is open."),
            Err(status) => return self.close(&format!("The host would not serve the file: {status:?}")),
        };
        let signature = fnv(&bytes);
        if self.signature == Some(signature) {
            return;
        }
        self.signature = Some(signature);
        match Dictionary::from_bytes(&bytes) {
            Ok(dict) => {
                let same = self.dict.as_ref() == Some(&dict);
                if self.dict.is_some() && !(same || self.wrote) {
                    self.history.clear();
                    self.status = "The open file changed, and the painter read it again.".into();
                }
                self.wrote = false;
                self.dict = Some(dict);
                self.problem.clear();
                self.fit();
            }
            Err(e) => self.close(&format!("The open file is not a cloth dictionary: {e}")),
        }
    }

    fn close(&mut self, why: &str) {
        self.dict = None;
        self.signature = None;
        self.history.clear();
        self.selection.clear();
        self.problem = why.to_string();
        self.dirty = true;
    }

    /// Keep the entry, the level and the selection inside what the dictionary holds.
    fn fit(&mut self) {
        let Some(dict) = &self.dict else { return };
        self.entry = self.entry.min(dict.entries.len().saturating_sub(1));
        let count = self.positions().len();
        if self.selection.len() != count {
            self.selection = vec![false; count];
        }
        // The force fields start at what the cloth holds.
        let stored = self
            .cloth()
            .map(|c| ([c.force.x, c.force.y, c.force.z], c.negate_force.get()));
        if let Some((force, negate)) = stored {
            self.brush.force = force.map(|n| n.to_string());
            self.brush.negate = negate;
        }
        self.dirty = true;
    }

    fn cloth(&self) -> Option<&or_io::gta5::CharacterCloth> {
        cloth::cloth(self.dict.as_ref()?, self.entry)
    }

    fn positions(&self) -> Vec<[f32; 3]> {
        self.cloth()
            .map_or_else(Vec::new, |c| cloth::positions(c, self.lod))
    }

    fn selected(&self) -> usize {
        self.selection.iter().filter(|s| **s).count()
    }

    /// Run one stroke, keeping the dictionary as it was to take it back, and try saving the result.
    fn stroke(&mut self, what: Paint) {
        let before = self.dict.clone();
        let (entry, lod) = (self.entry, self.lod);
        let selection = self.selection.clone();
        let painted = match self.dict.as_mut().and_then(|d| d.entries.get_mut(entry)) {
            Some(Some(cloth)) => cloth::paint(cloth, lod, &selection, what),
            _ => Err("this entry holds no cloth".to_string()),
        };
        match painted {
            Ok(count) => {
                if let Some(before) = before {
                    self.history.push(before);
                    if self.history.len() > UNDO {
                        self.history.remove(0);
                    }
                }
                self.status = format!("Painted {count} vertices. {}", self.trial());
                self.dirty = true;
            }
            Err(why) => self.status = why,
        }
    }

    /// The trial export: whether the painted dictionary would save, and the host's reason if not.
    fn trial(&self) -> String {
        match self.dict.as_ref().map(|dict| dict.to_bytes()) {
            Some(Ok(bytes)) => format!("Trial export: {} bytes, it saves.", bytes.len()),
            Some(Err(e)) => format!("Trial export refused: {e}"),
            None => String::new(),
        }
    }

    fn undo(&mut self) {
        match self.history.pop() {
            Some(before) => {
                self.dict = Some(before);
                self.status = format!("Took the last stroke back. {}", self.trial());
                self.dirty = true;
            }
            None => self.status = "There is no stroke to take back.".to_string(),
        }
    }

    /// Hand the painted dictionary to the host, which reads it back and saves it before it keeps it.
    fn write(&mut self) {
        let Some(dict) = &self.dict else {
            self.status = "Nothing is open to write.".to_string();
            return;
        };
        let bytes = match dict.to_bytes() {
            Ok(bytes) => bytes,
            Err(e) => {
                self.status = format!("Trial export refused, so nothing was written: {e}");
                return;
            }
        };
        match write_file(&bytes) {
            Ok(()) => {
                self.wrote = true;
                self.status = format!(
                    "Wrote {} bytes as the open document. Save the document to keep them.",
                    bytes.len()
                );
            }
            Err(refusal) => {
                self.status = format!("The host refused the file: {}", refusal.why);
            }
        }
    }

    fn choose(&mut self, selection: impl FnOnce(&[[f32; 3]], &[bool]) -> Vec<bool>) {
        let at = self.positions();
        self.selection = selection(&at, &self.selection);
        self.dirty = true;
    }

    fn frame(&mut self) {
        self.sync();
        let Some(dict) = &self.dict else {
            ui::label(&self.problem);
            self.overlay();
            return;
        };

        let names: Vec<String> = dict
            .codes
            .iter()
            .enumerate()
            .map(|(i, code)| {
                let verts = dict
                    .entries
                    .get(i)
                    .and_then(|c| c.as_deref())
                    .map_or(0, |c| cloth::positions(c, 0).len());
                format!("{code:08x} ({verts} vertices at level 0)")
            })
            .collect();
        let items: Vec<&str> = names.iter().map(String::as_str).collect();
        let chosen = ui::list("entry", &items, u32::try_from(self.entry).ok());
        if let Some(entry) = chosen.map(|e| e as usize).filter(|e| *e != self.entry) {
            self.entry = entry;
            self.fit();
        }

        let levels: Vec<String> = (0..LODS)
            .map(|lod| {
                let count = self
                    .cloth()
                    .map_or(0, |c| cloth::positions(c, lod).len());
                match count {
                    0 => format!("Level {lod} (empty)"),
                    n => format!("Level {lod} ({n} vertices)"),
                }
            })
            .collect();
        let levels: Vec<&str> = levels.iter().map(String::as_str).collect();
        if let Some(lod) = ui::list("lod", &levels, Some(self.lod as u32)).map(|l| l as usize) {
            if lod != self.lod {
                self.lod = lod;
                self.fit();
            }
        }

        let at = self.positions();
        self.summary(&at);
        self.selecting(&at);
        self.painting();
        self.force();

        ui::label("");
        if ui::button("undo", "Undo the last stroke") {
            self.undo();
        }
        if ui::button("write", "Write to the open document") {
            self.write();
        }
        ui::label(&self.status);
        self.overlay();
    }

    fn summary(&self, at: &[[f32; 3]]) {
        let Some(cloth) = self.cloth() else {
            ui::label("This entry holds no cloth.");
            return;
        };
        if at.is_empty() {
            ui::label("This level holds no vertices.");
            return;
        }
        let runs = cloth::runs(cloth, self.lod);
        let describe = |what: &str, run: Option<&[f32]>| match run.and_then(cloth::range) {
            Some((lo, hi)) => format!("{what} {lo:.3} to {hi:.3} over {}", run.map_or(0, <[f32]>::len)),
            None => format!("{what} not stored"),
        };
        let pinnable = (0..at.len()).filter(|v| cloth::pinnable(cloth, *v)).count();
        ui::label(&format!(
            "{} vertices, {} selected, {pinnable} pinnable. {}; {}; {}.",
            at.len(),
            self.selected(),
            describe("Weight", runs.weight),
            describe("Inflation scale", runs.inflation),
            describe("Pin radius", runs.pin_radius),
        ));
    }

    fn selecting(&mut self, at: &[[f32; 3]]) {
        ui::label("Select by region, as fractions of the level's extent:");
        for (axis, name) in AXES.iter().enumerate() {
            self.region_lo[axis] = ui::slider(
                &format!("lo{axis}"),
                &format!("{name} from"),
                0.0,
                1.0,
                self.region_lo[axis],
            );
            self.region_hi[axis] = ui::slider(
                &format!("hi{axis}"),
                &format!("{name} to"),
                0.0,
                1.0,
                self.region_hi[axis],
            );
        }
        let (lo, hi) = (self.region_lo, self.region_hi);
        if ui::button("region", "Select the region") {
            self.choose(|at, _| cloth::in_region(at, lo, hi));
        }
        if ui::button("add", "Add the region to the selection") {
            self.choose(|at, now| {
                let more = cloth::in_region(at, lo, hi);
                now.iter().zip(&more).map(|(a, b)| *a || *b).collect()
            });
        }
        if ui::button("all", "Select every vertex") {
            self.choose(|at, _| vec![true; at.len()]);
        }
        if ui::button("invert", "Invert the selection") {
            self.choose(|_, now| now.iter().map(|s| !s).collect());
        }
        if ui::button("clear", "Clear the selection") {
            self.choose(|at, _| vec![false; at.len()]);
        }
        let typed = ui::text_field("indices", "Vertices, such as 0-20, 25", &self.indices);
        self.indices = typed;
        if ui::button("these", "Select these vertices") {
            match cloth::indices(&self.indices, at.len()) {
                Ok(found) => self.choose(|at, _| {
                    let mut chosen = vec![false; at.len()];
                    found.iter().for_each(|v| chosen[*v] = true);
                    chosen
                }),
                Err(why) => self.status = why,
            }
        }
    }

    fn painting(&mut self) {
        ui::label("Paint the selection, with numbers typed in:");
        let b = &mut self.brush;
        b.weight = ui::text_field("weight", "Weight", &b.weight);
        b.inflation = ui::text_field("inflation", "Inflation scale", &b.inflation);
        b.pin_radius = ui::text_field("pin_radius", "Pin radius", &b.pin_radius);
        b.axis = ui::list("axis", &AXES, Some(b.axis)).unwrap_or(b.axis);
        b.gradient_from = ui::text_field("from", "Gradient weight at the low end of the axis", &b.gradient_from);
        b.gradient_to = ui::text_field("to", "Gradient weight at the high end of the axis", &b.gradient_to);
        let brush = b.clone();
        if ui::button("paint_weight", "Paint the weight") {
            self.stroke_with(&brush.weight, "the weight", Paint::Weight);
        }
        if ui::button("paint_gradient", "Paint the weight gradient along the axis") {
            let ends = (
                number(&brush.gradient_from, "the gradient's low end"),
                number(&brush.gradient_to, "the gradient's high end"),
            );
            match ends {
                (Ok(from), Ok(to)) => self.stroke(Paint::WeightGradient {
                    axis: brush.axis as usize,
                    from,
                    to,
                }),
                (Err(why), _) | (_, Err(why)) => self.status = why,
            }
        }
        if ui::button("paint_inflation", "Paint the inflation scale") {
            self.stroke_with(&brush.inflation, "the inflation scale", Paint::Inflation);
        }
        if ui::button("paint_pin_radius", "Paint the pin radius") {
            self.stroke_with(&brush.pin_radius, "the pin radius", Paint::PinRadius);
        }
        if ui::button("pin", "Mark the selection pinnable") {
            self.stroke(Paint::Pinnable(true));
        }
        if ui::button("unpin", "Mark the selection not pinnable") {
            self.stroke(Paint::Pinnable(false));
        }
    }

    /// A stroke with the number typed in `text`, or the sentence that says it is no number.
    fn stroke_with(&mut self, text: &str, what: &str, make: fn(f32) -> Paint) {
        match number(text, what) {
            Ok(n) => self.stroke(make(n)),
            Err(why) => self.status = why,
        }
    }

    /// The force the cloth is simulated under: its vector and whether it is negated.
    fn force(&mut self) {
        let Some(cloth) = self.cloth() else { return };
        let stored = (
            [cloth.force.x, cloth.force.y, cloth.force.z],
            cloth.negate_force.get(),
        );
        ui::label(&format!(
            "The cloth's force is {:?}, negated: {}.",
            stored.0, stored.1
        ));
        let b = &mut self.brush;
        for (axis, name) in AXES.iter().enumerate() {
            b.force[axis] = ui::text_field(&format!("f{axis}"), &format!("Force {name}"), &b.force[axis]);
        }
        b.negate = ui::checkbox("negate", "Negate the force", b.negate);
        let (typed, negate) = (b.force.clone(), b.negate);
        if !ui::button("set_force", "Set the force") {
            return;
        }
        let mut force = [0.0f32; 3];
        for (axis, text) in typed.iter().enumerate() {
            match number(text, &format!("the force along {}", AXES[axis])) {
                Ok(n) => force[axis] = n,
                Err(why) => return self.status = why,
            }
        }
        let (entry, Some(dict)) = (self.entry, self.dict.as_mut()) else {
            return;
        };
        let before = dict.clone();
        let Some(Some(cloth)) = dict.entries.get_mut(entry) else {
            return;
        };
        (cloth.force.x, cloth.force.y, cloth.force.z) = (force[0], force[1], force[2]);
        cloth.negate_force = or_io::decl::Bool8::new(negate);
        self.history.push(before);
        if self.history.len() > UNDO {
            self.history.remove(0);
        }
        self.status = format!("Set the force. {}", self.trial());
        self.dirty = true;
    }

    /// Show the level's vertices over the viewport: coloured by weight, and the selected ones larger.
    fn overlay(&mut self) {
        let show = matches!(setting_value(SETTING_SHOW), Ok(SettingValue::Bool(true)));
        let size = match setting_value(SETTING_SIZE) {
            Ok(SettingValue::Number(n)) => n as f32,
            _ => 1.0,
        };
        let key = (show, size.to_bits());
        if self.dirty || self.last != Some(key) {
            let marks = if show { self.marks(size) } else { Vec::new() };
            let _ = set_overlay(&marks);
            self.dirty = false;
            self.last = Some(key);
        }
    }

    fn marks(&self, size: f32) -> Vec<Primitive> {
        let at = self.positions();
        let Some(cloth) = self.cloth() else {
            return Vec::new();
        };
        let weights = cloth::runs(cloth, self.lod).weight;
        // A mark is a fiftieth of the level's longest side, so it reads at any scale of cloth.
        let side = (0..3)
            .filter_map(|a| cloth::range(&at.iter().map(|p| p[a]).collect::<Vec<_>>()))
            .map(|(lo, hi)| hi - lo)
            .fold(0.0f32, f32::max);
        let unit = (side / 50.0).max(1e-4) * size;
        at.iter()
            .enumerate()
            .map(|(v, p)| {
                let selected = self.selection.get(v).copied().unwrap_or(false);
                let colour = if selected {
                    [1.0, 0.9, 0.1, 1.0]
                } else {
                    weights
                        .and_then(|w| w.get(v))
                        .map_or([0.6, 0.6, 0.6, 1.0], |w| heat(*w))
                };
                Primitive::Point {
                    at: *p,
                    size: if selected { unit * 1.8 } else { unit },
                    colour,
                }
            })
            .collect()
    }
}

/// Blue at a weight of 0, red at 1.
fn heat(weight: f32) -> [f32; 4] {
    let t = weight.clamp(0.0, 1.0);
    [t, 0.2, 1.0 - t, 1.0]
}

/// FNV-1a over the bytes: enough to tell one file from another between frames.
fn fnv(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash ^ (bytes.len() as u64)
}

thread_local! {
    static MODEL: RefCell<Model> = RefCell::new(Model::new());
}

#[no_mangle]
pub extern "C" fn register() {
    register_panel(PANEL, "Cloth painter").expect("the panels grant buys this");
    register_command(
        COMMAND_WRITE,
        "Cloth painter: write the painted cloth to the open document",
        None,
    )
    .expect("the commands grant buys this");
    declare_setting(
        SETTING_SHOW,
        "Draw the level's vertices over the viewport",
        &SettingKind::Bool { default: true },
    )
    .expect("the settings grant buys this");
    declare_setting(
        SETTING_SIZE,
        "Size of a vertex mark",
        &SettingKind::Number {
            min: 0.1,
            max: 10.0,
            default: 1.0,
        },
    )
    .expect("the settings grant buys this");
}

#[no_mangle]
pub extern "C" fn draw_panel(index: i32) {
    assert_eq!(index, 0, "the painter registers one panel");
    MODEL.with(|model| model.borrow_mut().frame());
}

#[no_mangle]
pub extern "C" fn run_command(index: i32) {
    assert_eq!(index, 0, "the painter registers one command");
    MODEL.with(|model| {
        let mut model = model.borrow_mut();
        model.sync();
        model.write();
    });
}
