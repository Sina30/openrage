use or_io::gta5::data::timecycle::Timecycle;
use or_io::gta5::data::visual_settings::VisualSettings;

use crate::Vfs;

pub const TIME_VFS_PATH: &str = "common:/data/levels/gta5/time.xml";
pub const VISUAL_SETTINGS_VFS_PATH: &str = "common:/data/visualsettings.dat";

/// The weather cycles an install ships, each in a file of its own beside the others.
const CYCLES: [&str; 14] = [
    "extrasunny",
    "clear",
    "clearing",
    "clouds",
    "foggy",
    "neutral",
    "overcast",
    "rain",
    "smog",
    "snow",
    "snowlight",
    "thunder",
    "xmas",
    "blizzard",
];

/// What an install says of the look of the day: its time table and weather cycles, and its tuning values.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimecycleData {
    pub timecycle: Timecycle,
    pub visual_settings: VisualSettings,
}

/// Read the time table, the weather cycles and the visual settings the install holds; whatever it does not hold is left empty.
pub fn load_timecycle(vfs: &Vfs) -> TimecycleData {
    let text = |path: &str| {
        vfs.open(path)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    };
    let Some(time) = text(TIME_VFS_PATH) else {
        tracing::debug!(path = TIME_VFS_PATH, "no time table in the install");
        return TimecycleData::default();
    };
    let mut timecycle = Timecycle::new(&time);
    for name in CYCLES {
        match text(&format!("common:/data/timecycle/w_{name}.xml")) {
            Some(cycle) => timecycle.add_cycles(&cycle),
            None => tracing::debug!(cycle = name, "no weather cycle of this name"),
        }
    }
    let visual_settings = text(VISUAL_SETTINGS_VFS_PATH)
        .map(|text| VisualSettings::parse(&text))
        .unwrap_or_default();
    TimecycleData {
        timecycle,
        visual_settings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfs::FileDevice;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[derive(Debug)]
    struct CannedDevice {
        files: HashMap<String, Vec<u8>>,
    }

    impl FileDevice for CannedDevice {
        fn open(&self, relative_path: &str) -> Option<Vec<u8>> {
            self.files
                .get(&relative_path.trim_start_matches('/').to_ascii_lowercase())
                .cloned()
        }
    }

    fn vfs_of(files: &[(&str, &str)]) -> Vfs {
        let files = files
            .iter()
            .map(|(path, text)| (path.to_string(), text.as_bytes().to_vec()))
            .collect();
        let mut vfs = Vfs::new();
        vfs.mount("common:", Arc::new(CannedDevice { files }));
        vfs
    }

    const TIME: &str =
        r#"<time_data><sample name="a" hour="0"/><sample name="b" hour="12"/></time_data>"#;
    const CYCLE: &str = r#"<timecycle_keyframe_data><cycle name="CLEAR" regions="1">
        <region name="GLOBAL"><fog_start> 1 2 </fog_start></region></cycle></timecycle_keyframe_data>"#;

    #[test]
    fn the_cycles_the_install_holds_are_read_and_the_ones_it_lacks_are_left_out() {
        let vfs = vfs_of(&[
            ("data/levels/gta5/time.xml", TIME),
            ("data/timecycle/w_clear.xml", CYCLE),
            ("data/visualsettings.dat", "misc.MoonDimMult 0.3\n"),
        ]);
        let data = load_timecycle(&vfs);
        assert_eq!(data.timecycle.table.samples.len(), 2);
        assert_eq!(data.timecycle.cycles.len(), 1);
        assert!(data.timecycle.cycle("clear").is_some());
        assert_eq!(data.visual_settings.get_f32("misc.MoonDimMult", 1.0), 0.3);
    }

    #[test]
    fn an_install_with_no_time_table_gives_nothing_to_light_the_day_with() {
        let data = load_timecycle(&vfs_of(&[("data/timecycle/w_clear.xml", CYCLE)]));
        assert_eq!(data, TimecycleData::default());
    }
}
