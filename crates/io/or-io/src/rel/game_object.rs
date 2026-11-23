//! A game data file's items, typed: `rel_type` 151, such as
//! `game.dat151.rel`. Each item opens with its class id and name table offset
//! in one word, and lays its fields out at natural alignment unless its type
//! is marked `packed`. `rel_type` 149 and 150, older game data, read with the
//! same types; an older item stops before one of its type's `[newer]` fields.

use super::layout::{rel_items, rel_struct, NameTableOffset, Vector3};

rel_struct! {
    pub struct PedVoiceGroups {
        name_table_offset: NameTableOffset,
        pvg_type: u8,
        pvg_bits: u8,
        voice_priority: u8,
        ringtone_sounds: u32,
        primary_voice: Vec<PrimaryVoice> = u8,
        mini_voice: Vec<MiniVoice> = u8,
        gang_voice: Vec<GangVoice> = u8,
        backup_pvg: Vec<BackupPVG> = u8,
    }
}

rel_struct! {
    pub struct AnimalParams {
        name_table_offset: NameTableOffset,
        base_priority: i32,
        min_far_distance: f32,
        max_distance_to_bank_load: f32,
        max_sound_instances: u32,
        r#type: u8,
        scrape_vol_curve: u32,
        scrape_pitch_curve: u32,
        big_vehicle_impact: u32,
        vehicle_speed_for_big_impact: f32,
        run_over_sound: u32,
        context: Vec<Context> = u8,
    }
}

rel_struct! {
    pub struct AnimalVocalAnimTrigger {
        name_table_offset: NameTableOffset,
        anim_triggers: Vec<AnimTriggers> = u8,
    }
}

rel_struct! {
    pub struct AudBaseObject {
        name_table_offset: NameTableOffset,
        flags: u32,
    }
}

rel_struct! {
    pub struct VehicleCollisionSettings {
        name_table_offset: NameTableOffset,
        medium_intensity: u32,
        high_intensity: u32,
        small_scrape_impact: u32,
        slow_scrape_impact: u32,
        slow_scrape_loop: u32,
        roll_sound: u32,
        veh_on_veh_crash_sound: u32,
        high_impact_sweetener_sound: u32,
        sweetener_impact_threshold: f32,
        scrape_pitch_curve: u32,
        scrape_vol_curve: u32,
        slow_scrape_vol_curve: u32,
        scrape_impact_vol_curve: u32,
        slow_scrape_impact_curve: u32,
        impact_start_offset_curve: u32,
        impact_vol_curve: u32,
        vehicle_impact_start_offset_curve: u32,
        vehicle_impact_vol_curve: u32,
        velocity_impact_scaling_curve: u32,
        fake_impact_start_offset_curve: u32,
        fake_impact_vol_curve: u32,
        fake_impact_min: f32,
        fake_impact_max: f32,
        fake_impact_scale: f32,
        vehicle_size_scale: f32,
        fake_impact_trigger_delta: f32,
        fake_impact_sweetener_threshold: f32,
        damage_vol_curve: u32,
        jump_land_vol_curve: u32,
        vehicle_material_settings: u32,
        deformation_sound: u32,
        impact_debris: u32,
        glass_debris: u32,
        post_impact_debris: u32,
        ped_collision_min: f32,
        ped_collision_max: f32,
        collision_min: f32,
        collision_max: f32,
        vehicle_collision_min: f32,
        vehicle_collision_max: f32,
        vehicle_sweetener_threshold: f32,
        scrape_min: f32,
        scrape_max: f32,
        damage_min: f32,
        damage_max: f32,
        train_impact: u32,
        train_impact_loop: u32,
        wave_impact_loop: u32,
        frag_material: u32,
        wheel_frag_material: u32,
        melee_material_override: Vec<MeleeMaterialOverride> = u8,
    }
}

rel_struct! {
    pub struct TrailerAudioSettings {
        name_table_offset: NameTableOffset,
        bump_sound: u32,
        clatter_type: u8,
        link_stress_sound: u32,
        model_collision_settings: u32,
        fire_audio: u32,
        trailer_bump_volume_boost: i32,
        clatter_sensitivity_scalar: f32,
        clatter_volume_boost: i32,
        chassis_stress_sensitivity_scalar: f32,
        chassis_stress_volume_boost: i32,
        link_stress_volume_boost: i32,
        link_stress_sensitivity_scalar: f32,
    }
}

rel_struct! {
    pub struct CarAudioSettings {
        name_table_offset: NameTableOffset,
        flags: u32,
        engine: u32,
        granular_engine: u32,
        horn_sounds: u32,
        door_open_sound: u32,
        door_close_sound: u32,
        boot_open_sound: u32,
        boot_close_sound: u32,
        roll_sound: u32,
        brake_squeek_factor: f32,
        suspension_up: u32,
        suspension_down: u32,
        min_susp_comp_thresh: f32,
        max_susp_comp_thres: f32,
        vehicle_collisions: u32,
        car_make: u32,
        car_model: u32,
        car_category: u32,
        scanner_vehicle_settings: u32,
        jump_land_sound: u32,
        damaged_jump_land_sound: u32,
        jump_land_min_thresh: u32,
        jump_land_max_thresh: u32,
        volume_category: u8,
        gps_type: u8,
        radio_type: u8,
        radio_genre: u8,
        indicator_on: u32,
        indicator_off: u32,
        handbrake: u32,
        gps_voice: u8,
        ambient_radio_vol: i8,
        radio_leakage: u8,
        parking_tone: u32,
        roof_stuck_sound: u32,
        freeway_passby_tyre_bump_front: u32,
        freeway_passby_tyre_bump_back: u32,
        fire_audio: u32,
        startup_revs: u32,
        wind_noise: u32,
        freeway_passby_tyre_bump_front_side: u32,
        freeway_passby_tyre_bump_back_side: u32,
        max_roll_off_scale_player: f32,
        max_roll_off_scale_npc: f32,
        convertible_roof_sound_set: u32,
        off_road_rumble_sound_volume: i32,
        siren_sounds: u32,
        alternative_granular_engines: u32,
        alternative_granular_engine_probability: f32,
        stop_start_prob: u32,
        npc_road_noise: u32,
        npc_road_noise_highway: u32,
        forklift_sounds: u32,
        turret_sounds: u32,
        clatter_type: u8,
        digger_sounds: u32,
        tow_truck_sounds: u32,
        engine_type: u8,
        electric_engine: u32,
        openness: f32,
        reverse_warning_sound: u32,
        random_damage: u8,
        wind_cloth_sound: u32,
        car_specific_shutdown_sound: u32,
        clatter_sensitivity_scalar: f32,
        clatter_volume_boost: i32,
        chassis_stress_sensitivity_scalar: f32,
        chassis_stress_volume_boost: i32,
        [newer] {
            vehicle_rain_sound: u32,
            additional_revs_increase_smoothing: u16,
            additional_revs_decrease_smoothing: u16,
            additional_gear_change_smoothing: u16,
            additional_gear_change_smoothing_time: u16,
            convertible_roof_interior_sound_set: u32,
            vehicle_rain_sound_interior: u32,
            cabin_tone_loop: u32,
            interior_view_engine_openness: f32,
            jump_land_sound_interior: u32,
            damaged_jump_land_sound_interior: u32,
        }
    }
}

rel_struct! {
    pub struct VehicleEngineAudioSettings {
        name_table_offset: NameTableOffset,
        master_volume: i32,
        max_cone_attenuation: i32,
        fx_compensation: i32,
        non_player_fx_comp: i32,
        low_engine_loop: u32,
        high_engine_loop: u32,
        low_exhaust_loop: u32,
        high_exhaust_loop: u32,
        revs_off_loop: u32,
        min_pitch: i32,
        max_pitch: i32,
        idle_engine_simple_loop: u32,
        idle_exhaust_simple_loop: u32,
        idle_min_pitch: i32,
        idle_max_pitch: i32,
        induction_loop: u32,
        induction_min_pitch: i32,
        induction_max_pitch: i32,
        turbo_whine: u32,
        turbo_min_pitch: i32,
        turbo_max_pitch: i32,
        dump_valve: u32,
        dump_valve_prob: u32,
        turbo_spinup_speed: u32,
        gear_trans_loop: u32,
        gear_trans_min_pitch: i32,
        gear_trans_max_pitch: i32,
        gt_throttle_vol: u32,
        ignition: u32,
        engine_shutdown: u32,
        cooling_fan: u32,
        exhaust_pops: u32,
        start_loop: u32,
        master_turbo_volume: i32,
        master_transmission_volume: i32,
        engine_start_up: u32,
        engine_synth_def: u32,
        engine_synth_preset: u32,
        exhaust_synth_def: u32,
        exhaust_synth_preset: u32,
        engine_submix_voice: u32,
        exhaust_submix_voice: u32,
        upgraded_transmission_volume_boost: i32,
        upgraded_gear_change_int: u32,
        upgraded_gear_change_ext: u32,
        upgraded_engine_volume_boost_post_submix: i32,
        upgraded_engine_synth_def: u32,
        upgraded_engine_synth_preset: u32,
        upgraded_exhaust_volume_boost_post_submix: i32,
        upgraded_exhaust_synth_def: u32,
        upgraded_exhaust_synth_preset: u32,
        upgraded_dump_valve: u32,
        upgraded_turbo_volume_boost: i32,
        upgraded_gear_trans_loop: u32,
        upgraded_turbo_whine: u32,
        upgraded_induction_loop: u32,
        upgraded_exhaust_pops: u32,
    }
}

rel_struct! {
    pub struct CollisionMaterialSettings {
        name_table_offset: NameTableOffset,
        flags: u32,
        hard_impact: u32,
        solid_impact: u32,
        soft_impact: u32,
        scrape_sound: u32,
        ped_scrape_sound: u32,
        break_sound: u32,
        destroy_sound: u32,
        settle_sound: u32,
        bullet_impact_sound: u32,
        automatic_bullet_impact_sound: u32,
        shotgun_bullet_impact_sound: u32,
        big_vehicle_impact_sound: u32,
        ped_punch: u32,
        ped_kick: u32,
        medium_intensity: u32,
        high_intensity: u32,
        hardness: u8,
        min_impulse_mag: f32,
        max_impulse_mag: f32,
        impulse_mag_scalar: u16,
        max_scrape_speed: f32,
        min_scrape_speed: f32,
        scrape_impact_mag: f32,
        max_roll_speed: f32,
        min_roll_speed: f32,
        roll_impact_mag: f32,
        bullet_collision_scaling: u8,
        footstep_custom_impact_sound: u32,
        footstep_material_hardness: f32,
        footstep_material_loudness: f32,
        footstep_settings: u32,
        footstep_scaling: u8,
        scuffstep_scaling: u8,
        footstep_impact_scaling: u8,
        scuff_impact_scaling: u8,
        ski_settings: u32,
        animal_reference: u32,
        wet_material_reference: u32,
        impact_start_offset_curve: u32,
        impact_vol_curve: u32,
        scrape_pitch_curve: u32,
        scrape_vol_curve: u32,
        fast_tyre_roll: u32,
        detail_tyre_roll: u32,
        main_skid: u32,
        side_skid: u32,
        metal_shell_casing_small: u32,
        metal_shell_casing_medium: u32,
        metal_shell_casing_large: u32,
        metal_shell_casing_small_slow: u32,
        metal_shell_casing_medium_slow: u32,
        metal_shell_casing_large_slow: u32,
        plastic_shell_casing: u32,
        plastic_shell_casing_slow: u32,
        roll_sound: u32,
        rain_loop: u32,
        tyre_bump: u32,
        shockwave_sound: u32,
        random_ambient: u32,
        climb_settings: u32,
        dirtiness: f32,
        surface_settle: u32,
        ridged_surface_loop: u32,
        collision_count: u32,
        collision_count_threshold: u32,
        volume_threshold: f32,
        material_id: i32,
        sound_set: SoundSet,
        debris_launch: u32,
        debris_land: u32,
        off_road_sound: u32,
        roughness: f32,
        debris_on_slope: u32,
        bicycle_tyre_roll: u32,
        off_road_rumble_sound: u32,
        stealth_sweetener: u32,
        scuff: u32,
        material_type: u8,
        surface_priority: u8,
        wheel_spin_loop: u32,
        bicycle_tyre_grit_sound: u32,
        ped_slide_sound: u32,
        ped_roll_sound: u32,
        time_in_air_to_trigger_big_land: f32,
        melee_overide_material: u32,
        name_hash: u32,
        slow_mo_hard_impact: u32,
        slow_mo_bullet_impact_sound: u32,
        slow_mo_automatic_bullet_impact_sound: u32,
        slow_mo_shotgun_bullet_impact_sound: u32,
        slow_mo_bullet_impact_pre_suck: u32,
        slow_mo_bullet_impact_pre_suck_time: i32,
        slow_mo_automatic_bullet_impact_pre_suck: u32,
        slow_mo_automatic_bullet_impact_pre_suck_time: i32,
        slow_mo_shotgun_bullet_impact_pre_suck: u32,
        slow_mo_shotgun_bullet_impact_pre_suck_time: i32,
    }
}

rel_struct! {
    pub struct StaticEmitter {
        name_table_offset: NameTableOffset,
        flags: u32,
        sound: u32,
        radio_station: u32,
        position: Position,
        min_distance: f32,
        max_distance: f32,
        emitted_volume: i32,
        filter_cutoff: u16,
        hpf_cutoff: u16,
        rolloff_factor: u16,
        interior_settings: u32,
        interior_room: u32,
        radio_station_for_score: u32,
        max_leakage: f32,
        min_leakage_distance: u16,
        max_leakage_distance: u16,
        alarm_settings: u32,
        on_break_one_shot: u32,
        max_path_depth: u8,
        small_reverb_send: u8,
        medium_reverb_send: u8,
        large_reverb_send: u8,
        min_time_minutes: u16,
        max_time_minutes: u16,
        broken_health: f32,
        undamaged_health: f32,
    }
}

rel_struct! {
    pub struct EntityEmitter {
        name_table_offset: NameTableOffset,
        flags: u32,
        sound: u32,
        max_distance: f32,
        business_hours_prob: f32,
        evening_prob: f32,
        night_prob: f32,
        cone_inner_angle: f32,
        cone_outer_angle: f32,
        cone_max_atten: f32,
        stop_after_loud_sound: u32,
        max_path_depth: u8,
        broken_health: f32,
    }
}

rel_struct! {
    pub struct HeliAudioSettings {
        name_table_offset: NameTableOffset,
        flags: u32,
        rotor_loop: u32,
        rear_rotor_loop: u32,
        exhaust_loop: u32,
        banking_loop: u32,
        cabin_tone_loop: u32,
        throttle_smooth_rate: f32,
        bank_angle_volume_curve: u32,
        bank_throttle_volume_curve: u32,
        bank_throttle_pitch_curve: u32,
        rotor_throttle_volume_curve: u32,
        rotor_throttle_pitch_curve: u32,
        rotor_throttle_gap_curve: u32,
        rear_rotor_throttle_volume_curve: u32,
        exhaust_throttle_volume_curve: u32,
        exhaust_throttle_pitch_curve: u32,
        rotor_cone_front_angle: u16,
        rotor_cone_rear_angle: u16,
        rotor_cone_atten: i16,
        rear_rotor_cone_front_angle: u16,
        rear_rotor_cone_rear_angle: u16,
        rear_rotor_cone_atten: i16,
        exhaust_cone_front_angle: u16,
        exhaust_cone_rear_angle: u16,
        exhaust_cone_atten: i16,
        filter1_throttle_resonance_curve: u32,
        filter2_throttle_resonance_curve: u32,
        banking_resonance_curve: u32,
        rotor_volume_startup_curve: u32,
        blade_volume_startup_curve: u32,
        rotor_pitch_startup_curve: u32,
        rear_rotor_volume_startup_curve: u32,
        exhaust_volume_startup_curve: u32,
        rotor_gap_startup_curve: u32,
        start_up_one_shot: u32,
        blade_cone_up_angle: u16,
        blade_cone_down_angle: u16,
        blade_cone_atten: i16,
        thump_cone_up_angle: u16,
        thump_cone_down_angle: u16,
        thump_cone_atten: i16,
        scanner_make: u32,
        scanner_model: u32,
        scanner_category: u32,
        scanner_vehicle_settings: u32,
        radio_type: u8,
        radio_genre: u8,
        door_open: u32,
        door_close: u32,
        door_limit: u32,
        damage_loop: u32,
        rotor_speed_to_trigger_speed_curve: u32,
        rotor_volume_post_submix: i16,
        engine_synth_def: u32,
        engine_synth_preset: u32,
        exhaust_synth_def: u32,
        exhaust_synth_preset: u32,
        engine_submix_voice: u32,
        exhaust_submix_voice: u32,
        rotor_low_freq_loop: u32,
        exhaust_pitch_startup_curve: u32,
        vehicle_collisions: u32,
        fire_audio: u32,
        distant_loop: u32,
        secondary_door_start_open: u32,
        secondary_door_start_close: u32,
        secondary_door_close: u32,
        secondary_door_limit: u32,
        suspension_up: u32,
        suspension_down: u32,
        min_susp_comp_thresh: f32,
        max_susp_comp_thres: f32,
        damage_one_shots: u32,
        damage_warning: u32,
        tail_break: u32,
        rotor_break: u32,
        rear_rotor_break: u32,
        cable_deploy: u32,
        hard_scrape: u32,
        engine_cooling: u32,
        altitude_warning: u32,
        health_to_damage_volume_curve: u32,
        health_below600_to_damage_volume_curve: u32,
        damage_below600_loop: u32,
        aircraft_warning_serious_damage_thresh: f32,
        aircraft_warning_critical_damage_thresh: f32,
        aircraft_warning_max_speed: f32,
        simple_sound_for_loading: u32,
        slow_mo_rotor_loop: u32,
        slow_mo_rotor_low_freq_loop: u32,
        [newer] {
            vehicle_rain_sound: u32,
            vehicle_rain_sound_interior: u32,
            start_up_fail_one_shot: u32,
        }
    }
}

rel_struct! {
    pub struct MeleeCombatSettings {
        name_table_offset: NameTableOffset,
        swipe_sound: u32,
        general_hit_sound: u32,
        ped_hit_sound: u32,
        ped_response_sound: u32,
        head_take_down: u32,
        body_take_down: u32,
        small_animal_hit_sound: u32,
        small_animal_response_sound: u32,
        big_animal_hit_sound: u32,
        big_animal_response_sound: u32,
        slow_mo_ped_hit_sound: u32,
        slow_mo_ped_response_sound: u32,
        slow_mo_head_take_down: u32,
        slow_mo_body_take_down: u32,
        slow_mo_small_animal_hit_sound: u32,
        slow_mo_small_animal_response_sound: u32,
        slow_mo_big_animal_hit_sound: u32,
        slow_mo_big_animal_response_sound: u32,
    }
}

rel_struct! {
    pub struct TriggeredSpeechContext: AudBaseObject {
        triggered_context_repeat_time: i32,
        primary_repeat_time: i32,
        primary_repeat_time_on_same_voice: i32,
        backup_repeat_time: i32,
        backup_repeat_time_on_same_voice: i32,
        time_can_next_use_triggered_context: u32,
        time_can_next_play_primary: u32,
        time_can_next_play_backup: u32,
        primary_speech_context: u32,
        backup_speech_context: Vec<BackupSpeechContext> = u8,
    }
}

rel_struct! {
    pub struct SpeechContext: AudBaseObject {
        context_name_p_hash: u32,
        repeat_time: i32,
        repeat_time_on_same_voice: i32,
        volume_type: u8,
        audibility: u8,
        gender_non_specific_version: u32,
        time_can_next_play: u32,
        priority: u8,
        fake_gesture: Vec<FakeGesture> = u8,
    }
}

rel_struct! {
    pub struct SpeechContextVirtual: SpeechContext {
        resolving_function: u8,
        speech_contexts: Vec<SpeechContextVirtualSpeechContexts> = u8,
    }
}

rel_struct! {
    pub struct SpeechParams: AudBaseObject {
        override_context_settings: u8,
        preload_timeout_in_ms: u32,
        requested_volume: u8,
        audibility: u8,
        repeat_time: i32,
        repeat_time_on_same_voice: i32,
    }
}

rel_struct! {
    pub struct SpeechContextList {
        name_table_offset: NameTableOffset,
        speech_contexts: Vec<SpeechContextListSpeechContexts> = u8,
    }
}

rel_struct! {
    pub struct BoatAudioSettings {
        name_table_offset: NameTableOffset,
        flags: u32,
        engine1_loop: u32,
        engine1_vol: u32,
        engine1_pitch: u32,
        engine2_loop: u32,
        engine2_vol: u32,
        engine2_pitch: u32,
        low_reso_loop: u32,
        low_reso_loop_vol: u32,
        low_reso_pitch: u32,
        reso_loop: u32,
        reso_loop_vol: u32,
        reso_pitch: u32,
        water_turbulance: u32,
        water_turbulance_vol: u32,
        water_turbulance_pitch: u32,
        scanner_make: u32,
        scanner_model: u32,
        scanner_category: u32,
        scanner_vehicle_settings: u32,
        radio_type: u8,
        radio_genre: u8,
        horn_loop: u32,
        ignition_one_shot: u32,
        shutdown_one_shot: u32,
        engine_vol_post_submix: i16,
        exhaust_vol_post_submix: i16,
        engine_synth_def: u32,
        engine_synth_preset: u32,
        exhaust_synth_def: u32,
        exhaust_synth_preset: u32,
        vehicle_collisions: u32,
        engine_submix_voice: u32,
        exhaust_submix_voice: u32,
        wave_hit_sound: u32,
        left_water_sound: u32,
        idle_hull_slap_loop: u32,
        idle_hull_slap_speed_to_vol: u32,
        granular_engine: u32,
        bank_spray_sound: u32,
        ignition_loop: u32,
        engine_start_up: u32,
        sub_turning_engine_pitch_modifier: u32,
        sub_turning_sweetener_sound: u32,
        revs_to_sweetener_volume: u32,
        turning_to_sweetener_volume: u32,
        turning_to_sweetener_pitch: u32,
        dry_land_scrape: u32,
        max_roll_off_scale_player: f32,
        max_roll_off_scale_npc: f32,
        openness: f32,
        dry_land_hard_scrape: u32,
        dry_land_hard_impact: u32,
        wind_cloth_sound: u32,
        fire_audio: u32,
        door_open: u32,
        door_close: u32,
        door_limit: u32,
        door_start_close: u32,
        sub_extras_sound: u32,
        sfx_bank_sound: u32,
        submersible_creaks_sound: u32,
        wave_hit_big_air_sound: u32,
        big_air_min_time: f32,
        vehicle_rain_sound: u32,
        vehicle_rain_sound_interior: u32,
    }
}

rel_struct! {
    pub struct WeaponSettings {
        name_table_offset: NameTableOffset,
        flags: u32,
        fire_sound: u32,
        suppressed_fire_sound: u32,
        auto_sound: u32,
        report_sound: u32,
        report_volume_delta: f32,
        report_predelay_delta: u32,
        tail_energy_value: f32,
        echo_sound: u32,
        suppressed_echo_sound: u32,
        shell_casing_sound: u32,
        swipe_sound: u32,
        general_strike_sound: u32,
        ped_strike_sound: u32,
        heft_sound: u32,
        put_down_sound: u32,
        rattle_sound: u32,
        rattle_land_sound: u32,
        pickup_sound: u32,
        shell_casing: u8,
        safety_on: u32,
        safety_off: u32,
        special_weapon_sound_set: u32,
        bank_sound: u32,
        interior_shot_sound: u32,
        reload_sounds: u32,
        into_cover_sound: u32,
        out_of_cover_sound: u32,
        bullet_impact_time_filter: u32,
        last_bullet_impact_time: u32,
        rattle_aim_sound: u32,
        small_animal_strike_sound: u32,
        big_animal_strike_sound: u32,
        slow_mo_fire_sound: u32,
        hit_ped_sound: u32,
        [newer] {
            slow_mo_fire_sound_presuck: u32,
            slow_mo_suppressed_fire_sound: u32,
            slow_mo_suppressed_fire_sound_presuck: u32,
            slow_mo_report_sound: u32,
            slow_mo_interior_shot_sound: u32,
            slow_mo_ped_strike_sound: u32,
            slow_mo_ped_strike_sound_presuck: u32,
            slow_mo_big_animal_strike_sound: u32,
            slow_mo_big_animal_strike_sound_presuck: u32,
            slow_mo_small_animal_strike_sound: u32,
            slow_mo_small_animal_strike_sound_presuck: u32,
            slow_mo_fire_sound_presuck_time: i32,
            slow_mo_suppressed_fire_sound_presuck_time: i32,
            slow_mo_ped_strike_sound_presuck_time: i32,
            slow_mo_big_animal_strike_sound_presuck_time: i32,
            slow_mo_small_animal_strike_sound_presuck_time: i32,
            super_slow_mo_fire_sound: u32,
            super_slow_mo_fire_sound_presuck: u32,
            super_slow_mo_suppressed_fire_sound: u32,
            super_slow_mo_suppressed_fire_sound_presuck: u32,
            super_slow_mo_report_sound: u32,
            super_slow_mo_interior_shot_sound: u32,
            super_slow_mo_ped_strike_sound: u32,
            super_slow_mo_ped_strike_sound_presuck: u32,
            super_slow_mo_big_animal_strike_sound: u32,
            super_slow_mo_big_animal_strike_sound_presuck: u32,
            super_slow_mo_small_animal_strike_sound: u32,
            super_slow_mo_small_animal_strike_sound_presuck: u32,
            super_slow_mo_fire_sound_presuck_time: i32,
            super_slow_mo_suppressed_fire_sound_presuck_time: i32,
            super_slow_mo_ped_strike_sound_presuck_time: i32,
            super_slow_mo_big_animal_strike_sound_presuck_time: i32,
            super_slow_mo_small_animal_strike_sound_presuck_time: i32,
        }
    }
}

rel_struct! {
    pub struct ShoeAudioSettings {
        name_table_offset: NameTableOffset,
        walk: u32,
        dirty_walk: u32,
        creaky_walk: u32,
        glassy_walk: u32,
        run: u32,
        dirty_run: u32,
        creaky_run: u32,
        glassy_run: u32,
        wet_walk: u32,
        wet_run: u32,
        soft_walk: u32,
        scuff: u32,
        land: u32,
        shoe_hardness: f32,
        shoe_creakiness: f32,
        shoe_type: u8,
        shoe_fitness: f32,
        ladder_shoe_down: u32,
        ladder_shoe_up: u32,
        kick: u32,
    }
}

rel_struct! {
    pub struct ModelPhysicsParams {
        name_table_offset: NameTableOffset,
        num_feet: u32,
        ped_type: u8,
        footstep_tuning_values: u32,
        stop_speed_threshold: f32,
        walk_speed_threshold: f32,
        run_speed_threshold: f32,
        modes: Vec<ModelPhysicsParamsModes> = u8,
    }
}

rel_struct! {
    pub struct SkiAudioSettings {
        name_table_offset: NameTableOffset,
        straight_sound: u32,
        min_speed: f32,
        max_speed: f32,
        turn_sound: u32,
        min_turn: f32,
        max_turn: f32,
        turn_edge_sound: u32,
        min_slope_edge: f32,
        max_slope_edge: f32,
    }
}

rel_struct! {
    pub struct RadioStationList {
        name_table_offset: NameTableOffset,
        station: Vec<Station> = u8,
    }
}

rel_struct! {
    pub struct RadioStationSettings {
        name_table_offset: NameTableOffset,
        flags: u32,
        order: i32,
        next_station_settings_ptr: u32,
        genre: u8,
        ambient_radio_vol: i8,
        name: [u8; 32],
        track_list: Vec<RadioStationSettingsTrackList> = u32,
    }
}

rel_struct! {
    pub struct RadioStationTrackList {
        name_table_offset: NameTableOffset,
        flags: u32,
        category: u8,
        next_valid_selection_time: u32,
        history_write_index: u8,
        history_space: Vec<u32> = u8,
        total_num_tracks: u16,
        next_track_list_ptr: u32,
        track: Vec<RadioStationTrackListTrack> = u8,
    }
}

rel_struct! {
    pub struct RadioTrackCategoryData {
        name_table_offset: NameTableOffset,
        category: Vec<Category> = u8,
    }
}

rel_struct! {
    pub struct ScannerCrimeReport {
        name_table_offset: NameTableOffset,
        generic_instruction_snd_ref: u32,
        peds_around_instruction_snd_ref: u32,
        police_around_instruction_snd_ref: u32,
        acknowledge_situation_probability: f32,
        small_crime_sound_ref: u32,
        crime_set: Vec<CrimeSet> = u8,
    }
}

rel_struct! {
    pub struct PedRaceToPVG: AudBaseObject {
        universal: u32,
        white: u32,
        black: u32,
        chinese: u32,
        latino: u32,
        arab: u32,
        balkan: u32,
        jamaican: u32,
        korean: u32,
        italian: u32,
        pakistani: u32,
        friend_pv_gs: Vec<FriendPVGs> = u8,
    }
}

rel_struct! {
    pub struct FriendGroup: AudBaseObject {
    }
}

rel_struct! {
    pub struct StaticEmitterList {
        name_table_offset: NameTableOffset,
        emitter: Vec<StaticEmitterListEmitter> = u16,
    }
}

rel_struct! {
    pub struct ScriptedScannerLine {
        name_table_offset: NameTableOffset,
        flags: u32,
        phrase: Vec<Phrase> = u16,
    }
}

rel_struct! {
    pub struct ScannerSpecificLocation {
        name_table_offset: NameTableOffset,
        position: Vector3,
        radius: f32,
        prob_of_playing: f32,
        sounds: Vec<Sounds> = u8,
    }
}

rel_struct! {
    pub struct ScannerSpecificLocationList {
        name_table_offset: NameTableOffset,
        location: Vec<Location> = u16,
    }
}

rel_struct! {
    pub struct AmbientZone {
        name_table_offset: NameTableOffset,
        flags: u32,
        shape: u8,
        activation_zone: ActivationZone,
        positioning_zone: PositioningZone,
        built_up_factor: f32,
        min_ped_density: f32,
        max_ped_density: f32,
        ped_density_tod: u32,
        ped_density_scalar: f32,
        max_wind_influence: f32,
        min_wind_influence: f32,
        wind_elevation_sounds: u32,
        environment_rule: u32,
        audio_scene: u32,
        underwater_creak_factor: f32,
        ped_walla_settings: u32,
        randomised_radio_settings: u32,
        num_rules_to_play: u8,
        zone_water_calculation: u8,
        rule: Vec<Rule> = u8,
        dir_ambience: Vec<DirAmbience> = u8,
    }
}

rel_struct! {
    pub struct AmbientRule {
        name_table_offset: NameTableOffset,
        flags: u32,
        explicit_spawn_position: Vector3,
        sound: u32,
        category: u32,
        last_play_time: u32,
        dynamic_bank_id: i32,
        dynamic_slot_type: u32,
        weight: f32,
        min_dist: f32,
        max_dist: f32,
        min_time_minutes: u16,
        max_time_minutes: u16,
        min_repeat_time: u16,
        min_repeat_time_variance: u16,
        spawn_height: u8,
        explicit_spawn_position_usage: u8,
        max_local_instances: i8,
        max_global_instances: i8,
        blockability_factor: u8,
        max_path_depth: u8,
        condition: Vec<Condition> = u8,
    }
}

rel_struct! {
    pub struct AmbientZoneList {
        name_table_offset: NameTableOffset,
        zone: Vec<Zone> = u16,
    }
}

rel_struct! {
    pub struct AmbientSlotMap {
        name_table_offset: NameTableOffset,
        slot_map: Vec<SlotMap> = u8,
    }
}

rel_struct! {
    pub struct AmbientBankMap {
        name_table_offset: NameTableOffset,
        bank_map: Vec<BankMap> = u8,
    }
}

rel_struct! {
    pub struct EnvironmentRule: AudBaseObject {
        reverb_small: f32,
        reverb_medium: f32,
        reverb_large: f32,
        reverb_damp: f32,
        echo_delay: f32,
        echo_delay_variance: f32,
        echo_attenuation: f32,
        echo_number: u8,
        echo_sound_list: u32,
        base_echo_volume_modifier: f32,
    }
}

rel_struct! {
    pub struct InteriorSettings {
        name_table_offset: NameTableOffset,
        flags: u32,
        interior_walla_sound_set: u32,
        interior_reflections: u32,
        room: Vec<Room> = u8,
    }
}

rel_struct! {
    pub struct InteriorWeaponMetrics {
        name_table_offset: NameTableOffset,
        wetness: f32,
        visability: f32,
        lpf: u32,
        predelay: u32,
        hold: f32,
    }
}

rel_struct! {
    pub struct InteriorRoom {
        name_table_offset: NameTableOffset,
        flags: u32,
        room_name: u32,
        ambient_zone: u32,
        interior_type: u8,
        reverb_small: f32,
        reverb_medium: f32,
        reverb_large: f32,
        room_tone_sound: u32,
        rain_type: u8,
        exterior_audibility: f32,
        room_occlusion_damping: f32,
        non_marked_portal_occlusion: f32,
        distance_from_portal_for_occlusion: f32,
        distance_from_portal_fade_distance: f32,
        weapon_metrics: u32,
        interior_walla_sound_set: u32,
    }
}

rel_struct! {
    pub struct DoorAudioSettings {
        name_table_offset: NameTableOffset,
        sounds: u32,
        tuning_params: u32,
        max_occlusion: f32,
    }
}

rel_struct! {
    pub struct DoorTuningParams {
        name_table_offset: NameTableOffset,
        flags: u32,
        open_thresh: f32,
        heading_thresh: f32,
        closed_thresh: f32,
        speed_thresh: f32,
        speed_scale: f32,
        heading_delta_threshold: f32,
        angular_velocity_threshold: f32,
        door_type: u8,
    }
}

rel_struct! {
    pub struct DoorList {
        name_table_offset: NameTableOffset,
        door: Vec<Door> = u16,
    }
}

rel_struct! {
    pub struct ItemAudioSettings {
        name_table_offset: NameTableOffset,
        weapon_settings: u32,
        context_settings: Vec<ContextSettings> = u8,
    }
}

rel_struct! {
    pub struct ClimbingAudioSettings {
        name_table_offset: NameTableOffset,
        climb_launch: u32,
        climb_foot: u32,
        climb_knee: u32,
        climb_scrape: u32,
        climb_hand: u32,
    }
}

rel_struct! {
    pub struct ModelAudioCollisionSettings {
        name_table_offset: NameTableOffset,
        flags: u32,
        last_frag_time: u32,
        medium_intensity: u32,
        high_intensity: u32,
        break_sound: u32,
        destroy_sound: u32,
        uproot_sound: u32,
        wind_sounds: u32,
        swing_sound: u32,
        min_swing_vel: f32,
        maxswing_vel: f32,
        rain_loop: u32,
        shockwave_sound: u32,
        random_ambient: u32,
        entity_resonance: u32,
        weight: u8,
        material_override: Vec<MaterialOverride> = u8,
        frag_component_setting: Vec<FragComponentSetting> = u32,
    }
}

rel_struct! {
    pub struct TrainAudioSettings {
        name_table_offset: NameTableOffset,
        drive_tone: u32,
        drive_tone_synth: u32,
        idle_loop: u32,
        brakes: u32,
        big_brake_release: u32,
        brake_release: u32,
        wheel_dry: u32,
        air_horn_one_shot: u32,
        bell_loop_crosing: u32,
        bell_loop_engine: u32,
        ambient_carriage: u32,
        ambient_rumble: u32,
        ambient_grind: u32,
        ambient_squeal: u32,
        carriage_pitch_curve: u32,
        carriage_volume_curve: u32,
        drive_tone_pitch_curve: u32,
        drive_tone_volume_curve: u32,
        drive_tone_synth_pitch_curve: u32,
        drive_tone_synth_volume_curve: u32,
        grind_pitch_curve: u32,
        grind_volume_curve: u32,
        train_idle_pitch_curve: u32,
        train_idle_volume_curve: u32,
        squeal_pitch_curve: u32,
        squeal_volume_curve: u32,
        scrape_speed_volume_curve: u32,
        wheel_volume_curve: u32,
        wheel_delay_curve: u32,
        rumble_volume_curve: u32,
        brake_velocity_pitch_curve: u32,
        brake_velocity_volume_curve: u32,
        brake_acceleration_pitch_curve: u32,
        brake_acceleration_volume_curve: u32,
        train_approach_track_rumble: u32,
        track_rumble_distance_to_intensity: u32,
        train_distance_to_roll_off_scale: u32,
        vehicle_collisions: u32,
        [newer] {
            shockwave_intensity_scale: f32,
            shockwave_radius_scale: f32,
        }
    }
}

rel_struct! {
    pub struct WeatherAudioSettings {
        name_table_offset: NameTableOffset,
        strength: f32,
        blustery: f32,
        temperature: f32,
        time_of_day_affects_temperature: f32,
        whistle_volume_offset: f32,
        wind_gust: u32,
        audio_scene: u32,
        wind_gust_end: u32,
        wind_sound: Vec<WindSound> = u8,
    }
}

rel_struct! {
    pub struct WeatherTypeAudioReference {
        name_table_offset: NameTableOffset,
        reference: Vec<WeatherTypeAudioReferenceReference> = u8,
    }
}

rel_struct! {
    pub struct BicycleAudioSettings {
        name_table_offset: NameTableOffset,
        chain_loop: u32,
        sprocket_sound: u32,
        re_pedal_sound: u32,
        gear_change_sound: u32,
        road_noise_volume_scale: f32,
        skid_volume_scale: f32,
        suspension_up: u32,
        suspension_down: u32,
        min_susp_comp_thresh: f32,
        max_susp_comp_thres: f32,
        jump_land_sound: u32,
        damaged_jump_land_sound: u32,
        jump_land_min_thresh: u32,
        jump_land_max_thresh: u32,
        vehicle_collisions: u32,
        bell_sound: u32,
        brake_block: u32,
        brake_block_wet: u32,
    }
}

rel_struct! {
    pub struct PlaneAudioSettings {
        name_table_offset: NameTableOffset,
        flags: u32,
        engine_loop: u32,
        exhaust_loop: u32,
        idle_loop: u32,
        distance_loop: u32,
        propellor_loop: u32,
        banking_loop: u32,
        engine_cone_front_angle: u16,
        engine_cone_rear_angle: u16,
        engine_cone_atten: i16,
        exhaust_cone_front_angle: u16,
        exhaust_cone_rear_angle: u16,
        exhaust_cone_atten: i16,
        propellor_cone_front_angle: u16,
        propellor_cone_rear_angle: u16,
        propellor_cone_atten: i16,
        engine_throttle_volume_curve: u32,
        engine_throttle_pitch_curve: u32,
        exhaust_throttle_volume_curve: u32,
        exhaust_throttle_pitch_curve: u32,
        propellor_throttle_volume_curve: u32,
        propellor_throttle_pitch_curve: u32,
        idle_throttle_volume_curve: u32,
        idle_throttle_pitch_curve: u32,
        distance_throttle_volume_curve: u32,
        distance_throttle_pitch_curve: u32,
        distance_volume_curve: u32,
        stall_warning: u32,
        door_open: u32,
        door_close: u32,
        door_limit: u32,
        landing_gear_deploy: u32,
        landing_gear_retract: u32,
        ignition: u32,
        tyre_squeal: u32,
        banking_throttle_volume_curve: u32,
        banking_throttle_pitch_curve: u32,
        banking_angle_vol_curve: u32,
        afterburner_loop: u32,
        banking_style: u8,
        afterburner_throttle_vol_curve: u32,
        engine_synth_def: u32,
        engine_synth_preset: u32,
        exhaust_synth_def: u32,
        exhaust_synth_preset: u32,
        engine_submix_voice: u32,
        exhaust_submix_voice: u32,
        propellor_break_one_shot: u32,
        wind_noise: u32,
        peeling_pitch_curve: u32,
        diving_factor: i16,
        max_dive_pitch: i16,
        dive_air_speed_threshold: i16,
        diving_rate_approaching_ground: i16,
        peeling_afterburner_pitch_scaling_factor: f32,
        rudder: u32,
        aileron: u32,
        elevator: u32,
        door_start_open: u32,
        door_start_close: u32,
        vehicle_collisions: u32,
        fire_audio: u32,
        engine_miss_fire: u32,
        is_real_lod_range: u32,
        suspension_up: u32,
        suspension_down: u32,
        min_susp_comp_thresh: f32,
        max_susp_comp_thres: f32,
        damage_engine_speed_volume_curve: u32,
        damage_engine_speed_pitch_curve: u32,
        damage_health_volume_curve: u32,
        turbine_wind_down: u32,
        jet_damage_loop: u32,
        afterburner_throttle_pitch_curve: u32,
        npc_engine_smooth_amount: f32,
        flyby_sound: u32,
        diving_sound: u32,
        diving_sound_pitch_factor: i16,
        diving_sound_volume_factor: i16,
        max_dive_sound_pitch: i16,
        radio_type: u8,
        radio_genre: u8,
        aircraft_warning_serious_damage_thresh: f32,
        aircraft_warning_critical_damage_thresh: f32,
        aircraft_warning_max_speed: f32,
        simple_sound_for_loading: u32,
        fly_away_sound: u32,
        [newer] {
            vehicle_rain_sound: u32,
            vehicle_rain_sound_interior: u32,
            cabin_tone_loop: u32,
            wave_hit_sound: u32,
            wave_hit_big_air_sound: u32,
            big_air_min_time: f32,
            left_water_sound: u32,
            idle_hull_slap_loop: u32,
            idle_hull_slap_speed_to_vol: u32,
            water_turbulence_sound: u32,
        }
    }
}

rel_struct! {
    pub struct StemMix {
        name_table_offset: NameTableOffset,
        stem1_volume: i16,
        stem2_volume: i16,
        stem3_volume: i16,
        stem4_volume: i16,
        stem5_volume: i16,
        stem6_volume: i16,
        stem7_volume: i16,
        stem8_volume: i16,
    }
}

rel_struct! {
    pub struct MusicTimingConstraint {
        name_table_offset: NameTableOffset,
    }
}

rel_struct! {
    pub struct MusicAction {
        name_table_offset: NameTableOffset,
        flags: u32,
        constrain: u8,
        num_timing_constraints: u32,
        timing_constraints: [TimingConstraints; 2],
        delay_time: f32,
    }
}

rel_struct! {
    pub struct InteractiveMusicMood: AudBaseObject {
        fade_in_time: i16,
        fade_out_time: i16,
        amb_music_ducking_vol: f32,
        stem_mixes: Vec<StemMixes> = u8,
    }
}

rel_struct! {
    pub struct StartTrackAction: MusicAction {
        song: u32,
        mood: u32,
        volume_offset: f32,
        fade_in_time: i32,
        fade_out_time: i32,
        start_offset_scalar: f32,
        last_song: u32,
        alt_songs: Vec<AltSongs> = u8,
    }
}

rel_struct! {
    pub struct StopTrackAction: MusicAction {
        fade_time: i32,
    }
}

rel_struct! {
    pub struct SetMoodAction: MusicAction {
        mood: u32,
        volume_offset: f32,
        fade_in_time: i32,
        fade_out_time: i32,
    }
}

rel_struct! {
    pub struct MusicEvent {
        name_table_offset: NameTableOffset,
        actions: Vec<Actions> = u8,
    }
}

rel_struct! {
    pub struct StartOneShotAction: MusicAction {
        sound: u32,
        predelay: i32,
        fade_in_time: i32,
        fade_out_time: i32,
        sync_marker: u8,
    }
}

rel_struct! {
    pub struct StopOneShotAction: MusicAction {
    }
}

rel_struct! {
    pub struct BeatConstraint: MusicTimingConstraint {
        valid_sixteenths: u16,
    }
}

rel_struct! {
    pub struct BarConstraint: MusicTimingConstraint {
        pattern_length: i32,
        valid_bar: i32,
    }
}

rel_struct! {
    pub struct DirectionalAmbience {
        name_table_offset: NameTableOffset,
        flags: u32,
        sound_north: u32,
        sound_east: u32,
        sound_south: u32,
        sound_west: u32,
        volume_smoothing: f32,
        time_to_volume: u32,
        occlusion_to_vol: u32,
        height_to_cut_off: u32,
        occlusion_to_cut_off: u32,
        built_up_factor_to_vol: u32,
        building_density_to_vol: u32,
        tree_density_to_vol: u32,
        water_factor_to_vol: u32,
        instance_volume_scale: f32,
        height_above_blanket_to_vol: u32,
        highway_factor_to_vol: u32,
        vehicle_count_to_vol: u32,
        max_distance_out_to_sea: f32,
    }
}

rel_struct! {
    pub struct GunfightConductorIntensitySettings {
        name_table_offset: NameTableOffset,
        common: Common,
        going_into_cover: GoingIntoCover,
        running_away: RunningAway,
        open_space: OpenSpace,
        objects_medium_intensity: u32,
        objects_high_intensity: u32,
        vehicles_medium_intensity: u32,
        vehicles_high_intensity: u32,
        ground_medium_intensity: u32,
        ground_high_intensity: u32,
    }
}

rel_struct! {
    pub struct ScannerVoiceParams {
        name_table_offset: NameTableOffset,
        no_prefix: NoPrefix,
        bright: Bright,
        light: Light,
        dark: Dark,
        extra_prefix: ExtraPrefix,
    }
}

rel_struct! {
    pub struct ScannerVehicleParams {
        name_table_offset: NameTableOffset,
        flags: u32,
        voice: Vec<Voice> = u8,
    }
}

rel_struct! {
    pub struct AudioRoadInfo {
        name_table_offset: NameTableOffset,
        road_name: u32,
        tyre_bump_distance: f32,
    }
}

rel_struct! {
    pub struct MicSettingsReference {
        name_table_offset: NameTableOffset,
        reference: Vec<MicSettingsReferenceReference> = u8,
    }
}

rel_struct! {
    pub struct MicrophoneSettings {
        name_table_offset: NameTableOffset,
        flags: u32,
        mic_type: u8,
        listener_parameters: Vec<ListenerParameters> = u8,
    }
}

rel_struct! {
    pub struct CarRecordingAudioSettings {
        name_table_offset: NameTableOffset,
        mixgroup: u32,
        vehicle_model_id: u32,
        event: Vec<Event> = u8,
        persistent_mixer_scenes: Vec<PersistentMixerScenes> = u8,
    }
}

rel_struct! {
    pub struct CarRecordingList {
        name_table_offset: NameTableOffset,
        car_recording: Vec<CarRecording> = u16,
    }
}

rel_struct! {
    pub struct AnimalFootstepSettings {
        name_table_offset: NameTableOffset,
        walk_and_trot: u32,
        gallop1: u32,
        gallop2: u32,
        scuff: u32,
        jump: u32,
        land: u32,
        land_hard: u32,
        clumsy: u32,
        slide_loop: u32,
        audio_event_loudness: u8,
    }
}

rel_struct! {
    pub struct AnimalFootstepReference {
        name_table_offset: NameTableOffset,
        animal_reference: Vec<AnimalReference> = u16,
    }
}

rel_struct! {
    pub struct ShoeList {
        name_table_offset: NameTableOffset,
        shoe: Vec<ShoeListShoe> = u16,
    }
}

rel_struct! {
    pub struct ClothAudioSettings {
        name_table_offset: NameTableOffset,
        flags: u32,
        impact_sound: u32,
        walk_sound: u32,
        run_sound: u32,
        sprint_sound: u32,
        into_cover_sound: u32,
        out_of_cover_sound: u32,
        wind_sound: u32,
        intensity: f32,
        player_version: u32,
        bullet_impacts: u32,
        ped_roll_sound: u32,
        scrape_material_settings: u32,
        jump_land_sound: u32,
        melee_swing_sound: u32,
    }
}

rel_struct! {
    pub struct ClothList {
        name_table_offset: NameTableOffset,
        cloth: Vec<Cloth> = u16,
    }
}

rel_struct! {
    pub struct ExplosionAudioSettings {
        name_table_offset: NameTableOffset,
        flags: u32,
        explosion_sound: u32,
        debris_sound: u32,
        deafening_volume: f32,
        debris_time_scale: f32,
        debris_volume: f32,
        shockwave_intensity: f32,
        shockwave_delay: f32,
        slow_mo_explosion_sound: u32,
        slow_mo_explosion_pre_suck_sound: u32,
        slow_mo_explosion_pre_suck_sound_time: i32,
        slow_mo_explosion_pre_suck_mixer_scene: u32,
    }
}

rel_struct! {
    pub struct GranularEngineAudioSettings {
        name_table_offset: NameTableOffset,
        flags: u32,
        master_volume: i32,
        engine_accel: u32,
        exhaust_accel: u32,
        engine_volume_pre_submix: i32,
        exhaust_volume_pre_submix: i32,
        accel_volume_pre_submix: i32,
        decel_volume_pre_submix: i32,
        idle_volume_pre_submix: i32,
        engine_revs_volume_pre_submix: i32,
        exhaust_revs_volume_pre_submix: i32,
        engine_throttle_volume_pre_submix: i32,
        exhaust_throttle_volume_pre_submix: i32,
        engine_volume_post_submix: i32,
        exhaust_volume_post_submix: i32,
        engine_max_cone_attenuation: i32,
        exhaust_max_cone_attenuation: i32,
        engine_revs_volume_post_submix: i32,
        exhaust_revs_volume_post_submix: i32,
        engine_throttle_volume_post_submix: i32,
        exhaust_throttle_volume_post_submix: i32,
        gear_change_wobble_length: u32,
        gear_change_wobble_length_variance: f32,
        gear_change_wobble_speed: f32,
        gear_change_wobble_speed_variance: f32,
        gear_change_wobble_pitch: f32,
        gear_change_wobble_pitch_variance: f32,
        gear_change_wobble_volume: f32,
        gear_change_wobble_volume_variance: f32,
        engine_clutch_attenuation_post_submix: i32,
        exhaust_clutch_attenuation_post_submix: i32,
        engine_synth_def: u32,
        engine_synth_preset: u32,
        exhaust_synth_def: u32,
        exhaust_synth_preset: u32,
        npc_engine_accel: u32,
        npc_exhaust_accel: u32,
        rev_limiter_pop_sound: u32,
        min_rpm_override: u32,
        max_rpm_override: u32,
        engine_submix_voice: u32,
        exhaust_submix_voice: u32,
        exhaust_proximity_volume_post_submix: i32,
        rev_limiter_grains_to_play: u32,
        rev_limiter_grains_to_skip: u32,
        synchronised_synth: u32,
        upgraded_engine_volume_boost_post_submix: i32,
        upgraded_engine_synth_def: u32,
        upgraded_engine_synth_preset: u32,
        upgraded_exhaust_volume_boost_post_submix: i32,
        upgraded_exhaust_synth_def: u32,
        upgraded_exhaust_synth_preset: u32,
        damage_synth_hash_list: u32,
        upgraded_rev_limiter_pop: u32,
        engine_idle_volume_post_submix: i32,
        exhaust_idle_volume_post_submix: i32,
        startup_revs_volume_boost_engine_post_submix: i32,
        startup_revs_volume_boost_exhaust_post_submix: i32,
        [newer] {
            rev_limiter_apply_type: u8,
            rev_limiter_volume_cut: f32,
        }
    }
}

rel_struct! {
    pub struct ShoreLineAudioSettings {
        name_table_offset: NameTableOffset,
        flags: u32,
        activation_box: ActivationBox,
    }
}

rel_struct! {
    pub struct ShoreLinePoolAudioSettings: ShoreLineAudioSettings {
        water_lapping_min_delay: i32,
        water_lapping_max_delay: i32,
        water_splash_min_delay: i32,
        water_splash_max_delay: i32,
        first_quad_index: i32,
        second_quad_index: i32,
        third_quad_index: i32,
        fourth_quad_index: i32,
        smallest_distance_to_point: f32,
        shore_line_points: Vec<ShoreLinePoolAudioSettingsShoreLinePoints> = u16,
    }
}

rel_struct! {
    pub struct ShoreLineLakeAudioSettings: ShoreLineAudioSettings {
        next_shoreline: u32,
        lake_size: u8,
        shore_line_points: Vec<ShoreLineLakeAudioSettingsShoreLinePoints> = u8,
    }
}

rel_struct! {
    pub struct ShoreLineRiverAudioSettings: ShoreLineAudioSettings {
        next_shoreline: u32,
        river_type: u8,
        default_height: f32,
        shore_line_points: Vec<ShoreLineRiverAudioSettingsShoreLinePoints> = u8,
    }
}

rel_struct! {
    pub struct ShoreLineOceanAudioSettings: ShoreLineAudioSettings {
        ocean_type: u8,
        ocean_direction: u8,
        next_shoreline: u32,
        wave_start_dp_distance: f32,
        wave_start_height: f32,
        wave_breaks_dp_distance: f32,
        wave_breaks_height: f32,
        wave_end_dp_distance: f32,
        wave_end_height: f32,
        recede_height: f32,
        shore_line_points: Vec<ShoreLineOceanAudioSettingsShoreLinePoints> = u8,
    }
}

rel_struct! {
    pub struct ShoreLineList {
        name_table_offset: NameTableOffset,
        shore_lines: Vec<ShoreLines> = u16,
    }
}

rel_struct! {
    pub struct RadioTrackSettings {
        name_table_offset: NameTableOffset,
        sound: u32,
        history: History,
        start_offset: f32,
    }
}

rel_struct! {
    pub struct ModelFootStepTuning {
        name_table_offset: NameTableOffset,
        footstep_pitch_ratio_min: f32,
        footstep_pitch_ratio_max: f32,
        in_left_pocket_probability: f32,
        has_keys_probability: f32,
        has_money_probability: f32,
        modes: Vec<ModelFootStepTuningModes> = u8,
    }
}

rel_struct! {
    pub struct RadioDjSpeechAction: MusicAction {
        station: u32,
        category: u8,
    }
}

rel_struct! {
    pub struct SilenceConstraint: MusicTimingConstraint {
        minimum_duration: f32,
    }
}

rel_struct! {
    pub struct ReflectionsSettings {
        name_table_offset: NameTableOffset,
        flags: u32,
        min_delay: f32,
        max_delay: f32,
        delay_time_scalar: f32,
        delay_time_addition: f32,
        enter_sound: u32,
        exit_sound: u32,
        submix_voice: u32,
        smoothing: f32,
        post_submix_volume_attenuation: i16,
        roll_off_scale: f32,
        filter_mode: u8,
        filter_frequency_min: f32,
        filter_frequency_max: f32,
        filter_resonance_min: f32,
        filter_resonance_max: f32,
        filter_bandwidth_min: f32,
        filter_bandwidth_max: f32,
        distance_to_filter_input: u32,
    }
}

rel_struct! {
    pub struct AlarmSettings {
        name_table_offset: NameTableOffset,
        alarm_loop: u32,
        alarm_decay_curve: u32,
        stop_distance: u32,
        interior_settings: u32,
        bank_name: u32,
        centre_position: Vector3,
    }
}

rel_struct! {
    pub struct FadeOutRadioAction: MusicAction {
        fade_time: f32,
    }
}

rel_struct! {
    pub struct FadeInRadioAction: MusicAction {
        fade_time: f32,
    }
}

rel_struct! {
    pub struct ForceRadioTrackAction: MusicAction {
        station: u32,
        next_index: u32,
        track_list: Vec<ForceRadioTrackActionTrackList> = u8,
    }
}

rel_struct! {
    pub struct SlowMoSettings {
        name_table_offset: NameTableOffset,
        scene: u32,
        priority: u8,
        release: f32,
        slow_mo_sound: u32,
    }
}

rel_struct! {
    pub struct PedScenarioAudioSettings {
        name_table_offset: NameTableOffset,
        flags: u32,
        max_instances: u32,
        sound: u32,
        shared_ownership_radius: f32,
        prop_override: Vec<PropOverride> = u8,
    }
}

rel_struct! {
    pub struct PortalSettings {
        name_table_offset: NameTableOffset,
        max_occlusion: f32,
    }
}

rel_struct! {
    pub struct ElectricEngineAudioSettings {
        name_table_offset: NameTableOffset,
        master_volume: i32,
        speed_loop: u32,
        speed_loop_min_pitch: i32,
        speed_loop_max_pitch: i32,
        speed_loop_throttle_vol: i32,
        boost_loop: u32,
        boost_loop_min_pitch: i32,
        boost_loop_max_pitch: i32,
        boost_loop_spinup_speed: u32,
        boost_loop_vol: i32,
        revs_off_loop: u32,
        revs_off_loop_min_pitch: i32,
        revs_off_loop_max_pitch: i32,
        revs_off_loop_vol: i32,
        bank_load_sound: u32,
        engine_start_up: u32,
    }
}

rel_struct! {
    pub struct PlayerBreathingSettings {
        name_table_offset: NameTableOffset,
        time_between_low_run_breaths: i32,
        time_between_high_run_breaths: i32,
        time_between_exhausted_breaths: i32,
        time_between_final_breaths: i32,
        min_breath_state_change_wait_to_low: i32,
        max_breath_state_change_wait_to_low: i32,
        min_breath_state_change_low_to_high_from_wait: i32,
        max_breath_state_change_low_to_high_from_wait: i32,
        min_breath_state_change_high_to_low_from_low: i32,
        max_breath_state_change_high_to_low_from_low: i32,
        min_breath_state_change_low_to_high_from_high: i32,
        max_breath_state_change_low_to_high_from_high: i32,
        min_breath_state_change_exhausted_to_idle_from_low: i32,
        max_breath_state_change_exhausted_to_idle_from_low: i32,
        min_breath_state_change_exhausted_to_idle_from_high: i32,
        max_breath_state_change_exhausted_to_idle_from_high: i32,
        min_breath_state_change_low_to_high_from_exhausted: i32,
        max_breath_state_change_low_to_high_from_exhausted: i32,
    }
}

rel_struct! {
    pub struct PedWallaSpeechSettings {
        name_table_offset: NameTableOffset,
        speech_sound: u32,
        volume_above_rms_level: i16,
        max_volume: i16,
        ped_density_threshold: f32,
        speech_contexts: Vec<PedWallaSpeechSettingsSpeechContexts> = u8,
    }
}

rel_struct! {
    pub struct AircraftWarningSettings {
        name_table_offset: NameTableOffset,
        min_time_between_damage_reports: i32,
        targeted_lock: TargetedLock,
        missle_fired: MissleFired,
        acquiring_target: AcquiringTarget,
        target_acquired: TargetAcquired,
        all_clear: AllClear,
        plane_warning_stall: PlaneWarningStall,
        altitude_warning_low: AltitudeWarningLow,
        altitude_warning_high: AltitudeWarningHigh,
        engine_1_fire: Engine1Fire,
        engine_2_fire: Engine2Fire,
        engine_3_fire: Engine3Fire,
        engine_4_fire: Engine4Fire,
        damaged_serious: DamagedSerious,
        damaged_critical: DamagedCritical,
        overspeed: Overspeed,
        terrain: Terrain,
        pull_up: PullUp,
        low_fuel: LowFuel,
    }
}

rel_struct! {
    pub struct PedWallaSettingsList {
        name_table_offset: NameTableOffset,
        ped_walla_settings_instance: Vec<PedWallaSettingsInstance> = u8,
    }
}

rel_struct! {
    pub struct CopDispatchInteractionSettings {
        name_table_offset: NameTableOffset,
        min_time_between_interactions: i32,
        min_time_between_interactions_variance: i32,
        first_line_predelay: i32,
        first_line_predelay_variance: i32,
        second_line_predelay: i32,
        second_line_predelay_variance: i32,
        scanner_predelay: i32,
        scanner_predelay_variance: i32,
        min_time_between_spotted_and_vehicle_line_plays: i32,
        min_time_between_spotted_and_vehicle_line_plays_variance: i32,
        max_time_after_vehicle_change_spotted_line_can_play: i32,
        time_passed_since_last_line_to_force_voice_change: i32,
        num_cops_killed_to_force_voice_change: u32,
        suspect_crashed_vehicle: SuspectCrashedVehicle,
        suspect_entered_freeway: SuspectEnteredFreeway,
        suspect_entered_metro: SuspectEnteredMetro,
        suspect_is_in_car: SuspectIsInCar,
        suspect_is_on_foot: SuspectIsOnFoot,
        suspect_is_on_motorcycle: SuspectIsOnMotorcycle,
        suspect_left_freeway: SuspectLeftFreeway,
        request_backup: RequestBackup,
        acknowledge_situation: AcknowledgeSituation,
        unit_responding_dispatch: UnitRespondingDispatch,
        request_guidance_dispatch: RequestGuidanceDispatch,
        heli_mayday_dispatch: HeliMaydayDispatch,
        shot_at_heli: ShotAtHeli,
        heli_approaching_dispatch: HeliApproachingDispatch,
    }
}

rel_struct! {
    pub struct RadioTrackTextIds {
        name_table_offset: NameTableOffset,
        text_id: Vec<TextId> = u32,
    }
}

rel_struct! {
    pub struct RandomisedRadioEmitterSettings {
        name_table_offset: NameTableOffset,
        flags: u32,
        vehicle_emitter_bias: f32,
        static_emitter_config: StaticEmitterConfig,
        vehicle_emitter_config: VehicleEmitterConfig,
    }
}

rel_struct! {
    pub struct TennisVocalizationSettings {
        name_table_offset: NameTableOffset,
        lite: Lite,
        med: Med,
        strong: Strong,
    }
}

rel_struct! {
    pub struct DoorAudioSettingsLink {
        name_table_offset: NameTableOffset,
        door_audio_settings: u32,
    }
}

rel_struct! {
    pub struct SportsCarRevsSettings {
        name_table_offset: NameTableOffset,
        engine_volume_boost: i32,
        exhaust_volume_boost: i32,
        roll_off_boost: f32,
        min_trigger_time: u32,
        min_repeat_time: u32,
        attack_time_scalar: f32,
        release_time_scalar: f32,
        small_reverb_send: u8,
        medium_reverb_send: u8,
        large_reverb_send: u8,
        junction_trigger_speed: f32,
        junction_stop_speed: f32,
        junction_min_distance: u32,
        junction_max_distance: u32,
        passby_trigger_speed: f32,
        passby_stop_speed: f32,
        passby_min_distance: u32,
        passby_max_distance: u32,
        passby_lookahead_time: f32,
        cluster_trigger_distance: u32,
        cluster_trigger_speed: f32,
    }
}

rel_struct! {
    pub struct FoliageSettings {
        name_table_offset: NameTableOffset,
        run: u32,
        sprint: u32,
        walk: u32,
    }
}

rel_struct! {
    pub struct ReplayRadioStationTrackList {
        name_table_offset: NameTableOffset,
        flags: u32,
        track: Vec<ReplayRadioStationTrackListTrack> = u8,
    }
}

rel_struct! {
    pub struct MacsModelOverrideList {
        name_table_offset: NameTableOffset,
        entry: Vec<Entry> = u8,
    }
}

rel_struct! {
    pub struct MacsModelOverrideListBig {
        name_table_offset: NameTableOffset,
        entry: Vec<Entry> = u16,
    }
}

rel_struct! {
    pub struct SoundFieldDefinition: AudBaseObject {
        emitter: Vec<SoundFieldDefinitionEmitter> = u8,
        hull_indices: Vec<HullIndices> = u16,
    }
}

rel_struct! {
    pub struct GameObjectHashList: AudBaseObject {
        game_object_hashes: Vec<GameObjectHashes> = u32,
    }
}

rel_struct! {
    pub struct PrimaryVoice {
        voice_name: u32,
        reference_count: u32,
        running_tab: u32,
    }
}

rel_struct! {
    pub struct MiniVoice {
        voice_name: u32,
        reference_count: u32,
        running_tab: u32,
    }
}

rel_struct! {
    pub struct GangVoice {
        voice_name: u32,
        reference_count: u32,
        running_tab: u32,
    }
}

rel_struct! {
    pub struct BackupPVG {
        pvg: u32,
    }
}

rel_struct! {
    pub struct Context {
        context_name: [u8; 32],
        volume_offset: f32,
        roll_off: f32,
        priority: u32,
        probability: f32,
        min_time_between: i32,
        max_time_between: i32,
        max_distance: f32,
        context_bits: u8,
    }
}

rel_struct! {
    pub struct AnimTriggers {
        animal: u8,
        num_angry_contexts: u8,
        angry_contexts: [AngryContexts; 8],
        num_playful_contexts: u8,
        playful_contexts: [PlayfulContexts; 8],
    }
}

rel_struct! {
    pub struct AngryContexts {
        context: u32,
        weight: f32,
    }
}

rel_struct! {
    pub struct PlayfulContexts {
        context: u32,
        weight: f32,
    }
}

rel_struct! {
    pub struct MeleeMaterialOverride {
        material: u32,
        r#override: u32,
    }
}

rel_struct! {
    pub struct SoundSet {
        sound_set_ref: u32,
    }
}

rel_struct! {
    pub struct Position {
        x: f32,
        y: f32,
        z: f32,
    }
}

rel_struct! {
    pub struct BackupSpeechContext {
        speech_context: u32,
        weight: f32,
    }
}

rel_struct! {
    pub struct FakeGesture {
        gesture_enum: u8,
    }
}

rel_struct! {
    pub struct SpeechContextVirtualSpeechContexts {
        speech_context: u32,
    }
}

rel_struct! {
    pub struct SpeechContextListSpeechContexts {
        speech_context: u32,
    }
}

rel_struct! {
    pub struct ModelPhysicsParamsModes {
        name: u32,
        ground_distance_down_epsilon: f32,
        ground_distance_up_epsilon: f32,
        down_speed_epsilon: f32,
        hind_ground_distance_down_epsilon: f32,
        hind_ground_distance_up_epsilon: f32,
        speed_smoother_increase_rate: f32,
        speed_smoother_decrease_rate: f32,
        time_to_retrigger: u32,
    }
}

rel_struct! {
    pub struct Station {
        station_settings: u32,
    }
}

rel_struct! {
    pub struct RadioStationSettingsTrackList {
        track_list_ref: u32,
    }
}

rel_struct! {
    pub struct RadioStationTrackListTrack {
        context: u32,
        sound_ref: u32,
    }
}

rel_struct! {
    pub struct Category {
        category: u8,
        value: u32,
    }
}

rel_struct! {
    pub struct CrimeSet {
        sound_ref: u32,
        weight: f32,
    }
}

rel_struct! {
    pub struct FriendPVGs {
        pvg: u32,
    }
}

rel_struct! {
    pub struct StaticEmitterListEmitter {
        static_emitter: u32,
    }
}

rel_struct! {
    pub struct Phrase {
        sound_ref: u32,
        slot: u8,
        post_delay: u16,
    }
}

rel_struct! {
    pub struct Sounds {
        sound: u32,
    }
}

rel_struct! {
    pub struct Location {
        r#ref: u32,
    }
}

rel_struct! {
    pub struct ActivationZone {
        centre: Vector3,
        size: Vector3,
        post_rotation_offset: Vector3,
        size_scale: Vector3,
        rotation_angle: u16,
    }
}

rel_struct! {
    pub struct PositioningZone {
        centre: Vector3,
        size: Vector3,
        post_rotation_offset: Vector3,
        size_scale: Vector3,
        rotation_angle: u16,
    }
}

rel_struct! {
    pub struct Rule {
        r#ref: u32,
    }
}

rel_struct! {
    pub struct DirAmbience {
        r#ref: u32,
        volume: f32,
    }
}

rel_struct! {
    pub struct Condition {
        condition_variable: u32,
        condition_value: f32,
        condition_type: u8,
        bank_loading: u8,
    }
}

rel_struct! {
    pub struct Zone {
        r#ref: u32,
    }
}

rel_struct! {
    pub struct SlotMap {
        wave_slot_name: u32,
        slot_type: u32,
        priority: u32,
    }
}

rel_struct! {
    pub struct BankMap {
        bank_name: u32,
        slot_type: u32,
    }
}

rel_struct! {
    pub struct Room {
        r#ref: u32,
    }
}

rel_struct! {
    pub struct Door {
        prop_name: u32,
        door_audio_settings: u32,
    }
}

rel_struct! {
    pub struct ContextSettings {
        context: u32,
        weapon_settings: u32,
    }
}

rel_struct! {
    pub struct MaterialOverride {
        material: u32,
        r#override: u32,
    }
}

rel_struct! {
    pub struct FragComponentSetting {
        model_settings: u32,
    }
}

rel_struct! {
    pub struct WindSound {
        wind_sound_component: u32,
    }
}

rel_struct! {
    pub struct WeatherTypeAudioReferenceReference {
        weather_name: u32,
        weather_type: u32,
    }
}

rel_struct! {
    pub struct TimingConstraints {
        constraint: u32,
    }
}

rel_struct! {
    pub struct StemMixes {
        stem_mix: u32,
        on_trigger_action: u32,
        min_duration: f32,
        max_duration: f32,
        fade_time: f32,
        constrain: u8,
        num_timing_constraints: u8,
        timing_constraints: [TimingConstraints; 2],
    }
}

rel_struct! {
    pub struct AltSongs {
        song: u32,
        valid_area: u8,
    }
}

rel_struct! {
    pub struct Actions {
        action: u32,
    }
}

rel_struct! {
    pub struct Common {
        max_time_after_last_shot: u32,
    }
}

rel_struct! {
    pub struct GoingIntoCover {
        time_to_fake_bullet_impacts: f32,
        min_time_to_re_trigger_fake_bi: f32,
        max_time_to_re_trigger_fake_bi: f32,
    }
}

rel_struct! {
    pub struct RunningAway {
        min_time_to_fake_bullet_impacts: f32,
        max_time_to_fake_bullet_impacts: f32,
        vfx_probability: f32,
    }
}

rel_struct! {
    pub struct OpenSpace {
        min_time_to_fake_bullet_impacts: f32,
        max_time_to_fake_bullet_impacts: f32,
    }
}

rel_struct! {
    pub struct NoPrefix {
        black_sound: u32,
        blue_sound: u32,
        brown_sound: u32,
        beige_sound: u32,
        graphite_sound: u32,
        green_sound: u32,
        grey_sound: u32,
        orange_sound: u32,
        pink_sound: u32,
        red_sound: u32,
        silver_sound: u32,
        white_sound: u32,
        yellow_sound: u32,
    }
}

rel_struct! {
    pub struct Bright {
        black_sound: u32,
        blue_sound: u32,
        brown_sound: u32,
        beige_sound: u32,
        graphite_sound: u32,
        green_sound: u32,
        grey_sound: u32,
        orange_sound: u32,
        pink_sound: u32,
        red_sound: u32,
        silver_sound: u32,
        white_sound: u32,
        yellow_sound: u32,
    }
}

rel_struct! {
    pub struct Light {
        black_sound: u32,
        blue_sound: u32,
        brown_sound: u32,
        beige_sound: u32,
        graphite_sound: u32,
        green_sound: u32,
        grey_sound: u32,
        orange_sound: u32,
        pink_sound: u32,
        red_sound: u32,
        silver_sound: u32,
        white_sound: u32,
        yellow_sound: u32,
    }
}

rel_struct! {
    pub struct Dark {
        black_sound: u32,
        blue_sound: u32,
        brown_sound: u32,
        beige_sound: u32,
        graphite_sound: u32,
        green_sound: u32,
        grey_sound: u32,
        orange_sound: u32,
        pink_sound: u32,
        red_sound: u32,
        silver_sound: u32,
        white_sound: u32,
        yellow_sound: u32,
    }
}

rel_struct! {
    pub struct ExtraPrefix {
        battered: u32,
        beat_up: u32,
        chopped: u32,
        custom: u32,
        customized: u32,
        damaged: u32,
        dented: u32,
        dirty: u32,
        distressed: u32,
        mint: u32,
        modified: u32,
        run_down1: u32,
        run_down2: u32,
        rusty: u32,
    }
}

rel_struct! {
    pub struct Voice {
        manufacturer_sound: u32,
        model_sound: u32,
        category_sound: u32,
        scanner_color_override: u32,
    }
}

rel_struct! {
    pub struct MicSettingsReferenceReference {
        camera_type: u32,
        mic_settings: u32,
    }
}

rel_struct! {
    pub struct ListenerParameters {
        listener_contribution: f32,
        rear_attenuation_front_cone_angle: f32,
        rear_attenuation_rear_cone_angle: f32,
        close_rear_attenuation: f32,
        far_rear_attenuation: f32,
        roll_off: f32,
        rear_attenuation_type: u8,
        mic_length: f32,
        mic_to_player_local_environment_ratio: f32,
    }
}

rel_struct! {
    pub struct Event {
        time: f32,
        sound: u32,
        one_shot_scene: u32,
    }
}

rel_struct! {
    pub struct PersistentMixerScenes {
        scene: u32,
        start_time: f32,
        end_time: f32,
    }
}

rel_struct! {
    pub struct CarRecording {
        name: u32,
        car_recording_audio_settings: u32,
    }
}

rel_struct! {
    pub struct AnimalReference {
        name: u32,
        animal_footstep_settings: u32,
    }
}

rel_struct! {
    pub struct ShoeListShoe {
        shoe_name: u32,
        shoe_audio_settings: u32,
    }
}

rel_struct! {
    pub struct Cloth {
        cloth_name: u32,
        cloth_audio_settings: u32,
    }
}

rel_struct! {
    pub struct ActivationBox {
        center: Center,
        size: Size,
        rotation_angle: f32,
    }
}

rel_struct! {
    pub struct Center {
        x: f32,
        y: f32,
    }
}

rel_struct! {
    pub struct Size {
        width: f32,
        height: f32,
    }
}

rel_struct! {
    pub struct ShoreLinePoolAudioSettingsShoreLinePoints {
        x: f32,
        y: f32,
    }
}

rel_struct! {
    pub struct ShoreLineLakeAudioSettingsShoreLinePoints {
        x: f32,
        y: f32,
    }
}

rel_struct! {
    pub struct ShoreLineRiverAudioSettingsShoreLinePoints {
        x: f32,
        y: f32,
        river_width: f32,
    }
}

rel_struct! {
    pub struct ShoreLineOceanAudioSettingsShoreLinePoints {
        x: f32,
        y: f32,
    }
}

rel_struct! {
    pub struct ShoreLines {
        shore_line: u32,
    }
}

rel_struct! {
    pub struct History {
        category: u8,
        sound: u32,
    }
}

rel_struct! {
    pub struct ModelFootStepTuningModes {
        name: u32,
        shoe: ModelFootStepTuningModesShoe,
        dirt_layer: DirtLayer,
        creak_layer: CreakLayer,
        glass_layer: GlassLayer,
        wet_layer: WetLayer,
        custom_impact: CustomImpact,
        material_impact_impulse_scale: f32,
    }
}

rel_struct! {
    pub struct ModelFootStepTuningModesShoe {
        volume_offset: f32,
        lpf_cutoff: u32,
        attack_time: u32,
        drag_scuff_probability: f32,
    }
}

rel_struct! {
    pub struct DirtLayer {
        volume_offset: f32,
        lpf_cutoff: u32,
        attack_time: u32,
        sweetener_curve: u32,
    }
}

rel_struct! {
    pub struct CreakLayer {
        volume_offset: f32,
        lpf_cutoff: u32,
        attack_time: u32,
        sweetener_curve: u32,
    }
}

rel_struct! {
    pub struct GlassLayer {
        volume_offset: f32,
        lpf_cutoff: u32,
        attack_time: u32,
        sweetener_curve: u32,
    }
}

rel_struct! {
    pub struct WetLayer {
        volume_offset: f32,
        lpf_cutoff: u32,
        attack_time: u32,
        sweetener_curve: u32,
    }
}

rel_struct! {
    pub struct CustomImpact {
        volume_offset: f32,
        lpf_cutoff: u32,
        attack_time: u32,
    }
}

rel_struct! {
    pub struct ForceRadioTrackActionTrackList {
        track: u32,
    }
}

rel_struct! {
    pub struct PropOverride {
        prop_name: u32,
        sound: u32,
    }
}

rel_struct! {
    pub struct PedWallaSpeechSettingsSpeechContexts {
        context_name: [u8; 32],
        variations: i8,
    }
}

rel_struct! {
    pub struct TargetedLock {
        min_time_in_state_to_trigger: i32,
        max_time_between_trigger_and_play: i32,
        min_time_between_play: i32,
    }
}

rel_struct! {
    pub struct MissleFired {
        min_time_in_state_to_trigger: i32,
        max_time_between_trigger_and_play: i32,
        min_time_between_play: i32,
    }
}

rel_struct! {
    pub struct AcquiringTarget {
        min_time_in_state_to_trigger: i32,
        max_time_between_trigger_and_play: i32,
        min_time_between_play: i32,
    }
}

rel_struct! {
    pub struct TargetAcquired {
        min_time_in_state_to_trigger: i32,
        max_time_between_trigger_and_play: i32,
        min_time_between_play: i32,
    }
}

rel_struct! {
    pub struct AllClear {
        min_time_in_state_to_trigger: i32,
        max_time_between_trigger_and_play: i32,
        min_time_between_play: i32,
    }
}

rel_struct! {
    pub struct PlaneWarningStall {
        min_time_in_state_to_trigger: i32,
        max_time_between_trigger_and_play: i32,
        min_time_between_play: i32,
    }
}

rel_struct! {
    pub struct AltitudeWarningLow {
        min_time_in_state_to_trigger: i32,
        max_time_between_trigger_and_play: i32,
        min_time_between_play: i32,
        down_probe_length: f32,
    }
}

rel_struct! {
    pub struct AltitudeWarningHigh {
        min_time_in_state_to_trigger: i32,
        max_time_between_trigger_and_play: i32,
        min_time_between_play: i32,
    }
}

rel_struct! {
    pub struct Engine1Fire {
        min_time_in_state_to_trigger: i32,
        max_time_between_trigger_and_play: i32,
        min_time_between_play: i32,
    }
}

rel_struct! {
    pub struct Engine2Fire {
        min_time_in_state_to_trigger: i32,
        max_time_between_trigger_and_play: i32,
        min_time_between_play: i32,
    }
}

rel_struct! {
    pub struct Engine3Fire {
        min_time_in_state_to_trigger: i32,
        max_time_between_trigger_and_play: i32,
        min_time_between_play: i32,
    }
}

rel_struct! {
    pub struct Engine4Fire {
        min_time_in_state_to_trigger: i32,
        max_time_between_trigger_and_play: i32,
        min_time_between_play: i32,
    }
}

rel_struct! {
    pub struct DamagedSerious {
        min_time_in_state_to_trigger: i32,
        max_time_between_trigger_and_play: i32,
        min_time_between_play: i32,
    }
}

rel_struct! {
    pub struct DamagedCritical {
        min_time_in_state_to_trigger: i32,
        max_time_between_trigger_and_play: i32,
        min_time_between_play: i32,
    }
}

rel_struct! {
    pub struct Overspeed {
        min_time_in_state_to_trigger: i32,
        max_time_between_trigger_and_play: i32,
        min_time_between_play: i32,
    }
}

rel_struct! {
    pub struct Terrain {
        min_time_in_state_to_trigger: i32,
        max_time_between_trigger_and_play: i32,
        min_time_between_play: i32,
        forward_probe_length: f32,
    }
}

rel_struct! {
    pub struct PullUp {
        min_time_in_state_to_trigger: i32,
        max_time_between_trigger_and_play: i32,
        min_time_between_play: i32,
        max_time_since_terrain_trigger_to_play: i32,
    }
}

rel_struct! {
    pub struct LowFuel {
        min_time_in_state_to_trigger: i32,
        max_time_between_trigger_and_play: i32,
        min_time_between_play: i32,
    }
}

rel_struct! {
    pub struct PedWallaSettingsInstance {
        ped_walla_settings: u32,
        weight: f32,
        ped_density_threshold: f32,
        is_male: i8,
        is_gang: i8,
    }
}

rel_struct! {
    pub struct SuspectCrashedVehicle {
        min_time_between: i32,
        min_time_between_variance: i32,
    }
}

rel_struct! {
    pub struct SuspectEnteredFreeway {
        min_time_between: i32,
        min_time_between_variance: i32,
    }
}

rel_struct! {
    pub struct SuspectEnteredMetro {
        min_time_between: i32,
        min_time_between_variance: i32,
    }
}

rel_struct! {
    pub struct SuspectIsInCar {
        min_time_between: i32,
        min_time_between_variance: i32,
    }
}

rel_struct! {
    pub struct SuspectIsOnFoot {
        min_time_between: i32,
        min_time_between_variance: i32,
    }
}

rel_struct! {
    pub struct SuspectIsOnMotorcycle {
        min_time_between: i32,
        min_time_between_variance: i32,
    }
}

rel_struct! {
    pub struct SuspectLeftFreeway {
        min_time_between: i32,
        min_time_between_variance: i32,
    }
}

rel_struct! {
    pub struct RequestBackup {
        min_time_between: i32,
        min_time_between_variance: i32,
    }
}

rel_struct! {
    pub struct AcknowledgeSituation {
        min_time_after_crime: i32,
        max_time_after_crime: i32,
    }
}

rel_struct! {
    pub struct UnitRespondingDispatch {
        probability: f32,
    }
}

rel_struct! {
    pub struct RequestGuidanceDispatch {
        min_time_between: i32,
        min_time_between_variance: i32,
        time_not_spotted_to_trigger: i32,
        probability_of_megaphone_line: f32,
    }
}

rel_struct! {
    pub struct HeliMaydayDispatch {
        min_time_between: i32,
        min_time_between_variance: i32,
    }
}

rel_struct! {
    pub struct ShotAtHeli {
        min_time_between: i32,
        min_time_between_variance: i32,
    }
}

rel_struct! {
    pub struct HeliApproachingDispatch {
        min_time_between: i32,
        min_time_between_variance: i32,
    }
}

rel_struct! {
    pub struct TextId {
        offset_ms: u32,
        text_id: u32,
    }
}

rel_struct! {
    pub struct StaticEmitterConfig {
        static_emitter: u32,
        min_time: f32,
        max_time: f32,
        min_fade_radius: f32,
        max_fade_radius: f32,
        retrigger_time_min: u32,
        retrigger_time_max: u32,
    }
}

rel_struct! {
    pub struct VehicleEmitterConfig {
        static_emitter: u32,
        min_time: f32,
        max_time: f32,
        min_attack_time: u32,
        max_attack_time: u32,
        min_hold_time: u32,
        max_hold_time: u32,
        min_release_time: u32,
        max_release_time: u32,
        min_pan_angle_change: f32,
        max_pan_angle_change: f32,
        retrigger_time_min: u32,
        retrigger_time_max: u32,
    }
}

rel_struct! {
    pub struct Lite {
        null_weight: f32,
        lite_weight: f32,
        med_weight: f32,
        strong_weight: f32,
    }
}

rel_struct! {
    pub struct Med {
        null_weight: f32,
        lite_weight: f32,
        med_weight: f32,
        strong_weight: f32,
    }
}

rel_struct! {
    pub struct Strong {
        null_weight: f32,
        lite_weight: f32,
        med_weight: f32,
        strong_weight: f32,
    }
}

rel_struct! {
    pub struct ReplayRadioStationTrackListTrack {
        track_id: u32,
        sound_ref: u32,
    }
}

rel_struct! {
    pub struct Entry {
        model: u32,
        macs: u32,
    }
}

rel_struct! {
    pub struct SoundFieldDefinitionEmitter {
        spherical_coords: SphericalCoords,
    }
}

rel_struct! {
    pub struct SphericalCoords {
        azimuth: f32,
        zenith: f32,
    }
}

rel_struct! {
    pub struct HullIndices {
        index: u32,
    }
}

rel_struct! {
    pub struct GameObjectHashes {
        game_object_hash: u32,
    }
}

rel_items! {
    pub enum GameObjectItem, class byte 0 {
        1 => VehicleCollisionSettings,
        2 => TrailerAudioSettings,
        3 => CarAudioSettings,
        4 => VehicleEngineAudioSettings,
        5 => CollisionMaterialSettings,
        6 => StaticEmitter,
        7 => EntityEmitter,
        8 => HeliAudioSettings,
        9 => MeleeCombatSettings,
        11 => TriggeredSpeechContext,
        12 => SpeechContext,
        13 => SpeechContextVirtual,
        14 => SpeechParams,
        15 => SpeechContextList,
        16 => BoatAudioSettings,
        17 => WeaponSettings,
        18 => ShoeAudioSettings,
        22 => ModelPhysicsParams,
        23 => SkiAudioSettings,
        24 => RadioStationList,
        25 => RadioStationSettings,
        26 => RadioStationTrackList,
        27 => RadioTrackCategoryData,
        28 => ScannerCrimeReport,
        29 => PedRaceToPVG,
        30 => PedVoiceGroups packed,
        31 => FriendGroup,
        32 => StaticEmitterList,
        33 => ScriptedScannerLine,
        35 => ScannerSpecificLocation,
        36 => ScannerSpecificLocationList,
        37 => AmbientZone,
        38 => AmbientRule,
        39 => AmbientZoneList,
        40 => AmbientSlotMap,
        41 => AmbientBankMap,
        42 => EnvironmentRule,
        44 => InteriorSettings,
        45 => InteriorWeaponMetrics,
        46 => InteriorRoom,
        47 => DoorAudioSettings,
        48 => DoorTuningParams,
        49 => DoorList,
        50 => ItemAudioSettings,
        51 => ClimbingAudioSettings,
        52 => ModelAudioCollisionSettings,
        53 => TrainAudioSettings,
        54 => WeatherAudioSettings,
        55 => WeatherTypeAudioReference,
        56 => BicycleAudioSettings,
        57 => PlaneAudioSettings,
        59 => StemMix,
        62 => InteractiveMusicMood,
        63 => StartTrackAction,
        64 => StopTrackAction,
        65 => SetMoodAction,
        66 => MusicEvent,
        67 => StartOneShotAction,
        68 => StopOneShotAction,
        69 => BeatConstraint,
        70 => BarConstraint,
        71 => DirectionalAmbience,
        72 => GunfightConductorIntensitySettings,
        73 => AnimalParams packed,
        74 => AnimalVocalAnimTrigger packed,
        75 => ScannerVoiceParams,
        76 => ScannerVehicleParams,
        77 => AudioRoadInfo,
        78 => MicSettingsReference,
        79 => MicrophoneSettings,
        80 => CarRecordingAudioSettings,
        81 => CarRecordingList,
        82 => AnimalFootstepSettings,
        83 => AnimalFootstepReference,
        84 => ShoeList,
        85 => ClothAudioSettings,
        86 => ClothList,
        87 => ExplosionAudioSettings,
        88 => GranularEngineAudioSettings,
        90 => ShoreLinePoolAudioSettings,
        91 => ShoreLineLakeAudioSettings,
        92 => ShoreLineRiverAudioSettings,
        93 => ShoreLineOceanAudioSettings,
        94 => ShoreLineList,
        95 => RadioTrackSettings,
        96 => ModelFootStepTuning,
        98 => RadioDjSpeechAction,
        99 => SilenceConstraint,
        100 => ReflectionsSettings,
        101 => AlarmSettings,
        102 => FadeOutRadioAction,
        103 => FadeInRadioAction,
        104 => ForceRadioTrackAction,
        105 => SlowMoSettings,
        106 => PedScenarioAudioSettings,
        107 => PortalSettings,
        108 => ElectricEngineAudioSettings,
        109 => PlayerBreathingSettings,
        110 => PedWallaSpeechSettings,
        111 => AircraftWarningSettings,
        112 => PedWallaSettingsList,
        113 => CopDispatchInteractionSettings,
        114 => RadioTrackTextIds,
        115 => RandomisedRadioEmitterSettings,
        116 => TennisVocalizationSettings,
        117 => DoorAudioSettingsLink,
        118 => SportsCarRevsSettings,
        119 => FoliageSettings,
        120 => ReplayRadioStationTrackList,
        121 => MacsModelOverrideList,
        122 => MacsModelOverrideListBig,
        123 => SoundFieldDefinition,
        124 => GameObjectHashList,
    }
}
