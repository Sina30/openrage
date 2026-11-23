use std::sync::{Arc, Mutex, PoisonError};

use or_simulation::sinks::{
    AudioEvent, AudioSink, FogVolumeInfo, FogVolumeSink, LightInfo, LightSink,
};

#[derive(Debug, Default)]
pub struct EditorAudioSink;

impl AudioSink for EditorAudioSink {
    fn on_event(&self, event: AudioEvent<'_>) {
        match event {
            AudioEvent::Start { name, pos } => {
                tracing::info!(target: "ptfx-audio", "spawn-effect-start name={name} pos={pos:?}");
            }
            AudioEvent::Stop { name, pos } => {
                tracing::info!(target: "ptfx-audio", "spawn-effect-stop  name={name} pos={pos:?}");
            }
            AudioEvent::Recycle { name, pos } => {
                tracing::info!(target: "ptfx-audio", "spawn-effect-recycle name={name} pos={pos:?}");
            }
        }
    }
}

/// The lights the effects gave in the last update that ran: a paused effect keeps them.
#[derive(Debug, Default)]
pub struct EditorLightSink(Mutex<Vec<LightInfo>>);

impl EditorLightSink {
    /// What the last update lit with.
    pub fn lights(&self) -> Vec<LightInfo> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl LightSink for EditorLightSink {
    fn begin(&self) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    fn add_light(&self, info: LightInfo) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(info);
    }
}

/// The fog volumes the effects gave in the last update that ran: a paused effect keeps them.
#[derive(Debug, Default)]
pub struct EditorFogSink(Mutex<Vec<FogVolumeInfo>>);

impl EditorFogSink {
    /// What the last update filled the air with.
    pub fn volumes(&self) -> Vec<FogVolumeInfo> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl FogVolumeSink for EditorFogSink {
    fn begin(&self) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    fn add_fog_volume(&self, info: FogVolumeInfo) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(info);
    }
}

#[derive(Debug, Default)]
pub struct EditorSinks {
    pub audio: Arc<EditorAudioSink>,
    pub lights: Arc<EditorLightSink>,
    pub fog: Arc<EditorFogSink>,
}

impl EditorSinks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn install(&self, manager: &mut or_simulation::PtfxManager) {
        manager.sink_audio = Some(self.audio.clone());
        manager.sink_light = Some(self.lights.clone());
        manager.sink_fog_volume = Some(self.fog.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    fn light(x: f32) -> LightInfo {
        LightInfo {
            pos: Vec3::new(x, 0.0, 0.0),
            colour: Vec3::ONE,
            intensity: 1.0,
            range: 2.0,
            direction: Vec3::ZERO,
            cone_half_angle: 0.0,
            falloff: 1.0,
            corona_size: 0.0,
        }
    }

    fn fog(x: f32) -> FogVolumeInfo {
        FogVolumeInfo {
            pos: Vec3::new(x, 0.0, 0.0),
            scale: Vec3::ONE,
            rotation: Vec3::ZERO,
            range: 3.0,
            colour: glam::Vec4::ONE,
            density: 1.0,
            falloff: 1.0,
            hdr_mult: 1.0,
            lighting: or_simulation::sinks::FogLighting::FogHdr,
            use_ground_fog_colour: false,
            sort_with_particles: false,
        }
    }

    #[test]
    fn an_update_replaces_the_fog_of_the_one_before() {
        let sink = EditorFogSink::default();
        sink.begin();
        sink.add_fog_volume(fog(1.0));
        sink.add_fog_volume(fog(2.0));
        assert_eq!(sink.volumes().len(), 2);
        sink.begin();
        sink.add_fog_volume(fog(3.0));
        let kept = sink.volumes();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].pos.x, 3.0);
        sink.begin();
        assert!(
            sink.volumes().is_empty(),
            "an update with no fog leaves none"
        );
    }

    #[test]
    fn an_update_replaces_the_lights_of_the_one_before() {
        let sink = EditorLightSink::default();
        sink.begin();
        sink.add_light(light(1.0));
        sink.add_light(light(2.0));
        assert_eq!(sink.lights().len(), 2);
        sink.begin();
        sink.add_light(light(3.0));
        let kept = sink.lights();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].pos.x, 3.0);
        sink.begin();
        assert!(
            sink.lights().is_empty(),
            "an update with no light leaves none"
        );
    }
}
