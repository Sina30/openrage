//! The root of a weapon components file (`CWeaponComponentInfoBlob`) and the components and reload data it lists.

use super::weapon::*;
use super::*;

meta_record! {
    /// `CWeaponComponentInfoBlob`, as the install's weapon metadata states it.
    CWeaponComponentInfoBlob = "CWeaponComponentInfoBlob" {
        data: Vec<ComponentDataAny> = "Data",
        info_blob_name: String = "InfoBlobName",
        infos: Vec<ComponentInfoAny> = "Infos",
    }
}

meta_record! {
    /// `CWeaponComponentReloadData`, as the install's weapon metadata states it.
    CWeaponComponentReloadData = "CWeaponComponentReloadData" {
        anim_rate_modifier: f32 = "AnimRateModifier",
        name: String = "Name",
        ped_aim_reload_clip_id: String = "PedAimReloadClipId",
        ped_aim_reload_empty_clip_id: String = "PedAimReloadEmptyClipId",
        ped_idle_reload_clip_id: String = "PedIdleReloadClipId",
        ped_idle_reload_empty_clip_id: String = "PedIdleReloadEmptyClipId",
        ped_low_left_cover_reload_clip_id: String = "PedLowLeftCoverReloadClipId",
        ped_low_right_cover_reload_clip_id: String = "PedLowRightCoverReloadClipId",
        weapon_aim_reload_clip_id: String = "WeaponAimReloadClipId",
        weapon_aim_reload_empty_clip_id: String = "WeaponAimReloadEmptyClipId",
        weapon_idle_reload_clip_id: String = "WeaponIdleReloadClipId",
        weapon_idle_reload_empty_clip_id: String = "WeaponIdleReloadEmptyClipId",
        weapon_low_left_cover_reload_clip_id: String = "WeaponLowLeftCoverReloadClipId",
        weapon_low_right_cover_reload_clip_id: String = "WeaponLowRightCoverReloadClipId",
    }
}

meta_record! {
    /// `CWeaponSwapData`, as the install's weapon metadata states it.
    CWeaponSwapData = "CWeaponSwapData" {
        anim_playback_rate: f32 = "AnimPlaybackRate",
        name: String = "Name",
        ped_holster_clip_id: String = "PedHolsterClipId",
        ped_holster_cover_clip_id: String = "PedHolsterCoverClipId",
        ped_holster_crouch_clip_id: String = "PedHolsterCrouchClipId",
        ped_holster_crouch_discard_clip_id: String = "PedHolsterCrouchDiscardClipId",
        ped_holster_discard_clip_id: String = "PedHolsterDiscardClipId",
        ped_holster_weapon_clip_id: String = "PedHolsterWeaponClipId",
        ped_un_holster_clip_id: String = "PedUnHolsterClipId",
        ped_un_holster_crouch_clip_id: String = "PedUnHolsterCrouchClipId",
        ped_un_holster_left_cover_clip_id: String = "PedUnHolsterLeftCoverClipId",
        ped_un_holster_right_cover_clip_id: String = "PedUnHolsterRightCoverClipId",
        ped_un_holster_weapon_clip_id: String = "PedUnHolsterWeaponClipId",
    }
}

meta_record! {
    /// `CWeaponComponentReloadLoopedData`, as the install's weapon metadata states it; carried unread: Sections.
    CWeaponComponentReloadLoopedData = "CWeaponComponentReloadLoopedData" {
        name: String = "Name",
    }
}

meta_record! {
    /// `CWeaponComponentInfo`, as the install's weapon metadata states it; carried unread: AccuracyModifier, DamageModifier, FallOffModifier.
    CWeaponComponentInfo = "CWeaponComponentInfo" {
        apply_weapon_tint: bool = "ApplyWeaponTint",
        attach_bone: String = "AttachBone",
        create_object: bool = "CreateObject",
        hud_accuracy: i32 = "HudAccuracy",
        hud_capacity: i32 = "HudCapacity",
        hud_damage: i32 = "HudDamage",
        hud_range: i32 = "HudRange",
        hud_speed: i32 = "HudSpeed",
        loc_desc: String = "LocDesc",
        loc_name: String = "LocName",
        model: String = "Model",
        name: String = "Name",
        b_shown_on_wheel: bool = "bShownOnWheel",
    }
}

meta_record! {
    /// `CWeaponComponentClipInfo`, as the install's weapon metadata states it; carried unread: AccuracyModifier, DamageModifier, ReloadData.
    CWeaponComponentClipInfo = "CWeaponComponentClipInfo" {
        ammo_info: String = "AmmoInfo",
        attach_bone: String = "AttachBone",
        bullets_in_batch: i32 = "BulletsInBatch",
        clip_size: i32 = "ClipSize",
        create_object: bool = "CreateObject",
        hud_accuracy: i32 = "HudAccuracy",
        hud_capacity: i32 = "HudCapacity",
        hud_damage: i32 = "HudDamage",
        hud_range: i32 = "HudRange",
        hud_speed: i32 = "HudSpeed",
        loc_desc: String = "LocDesc",
        loc_name: String = "LocName",
        model: String = "Model",
        name: String = "Name",
        shell_fx: String = "ShellFx",
        shell_fx_fp: String = "ShellFxFP",
        tracer_fx: String = "TracerFx",
        tracer_fx_force_last: i32 = "TracerFxForceLast",
        tracer_fx_spacing: i32 = "TracerFxSpacing",
        b_shown_on_wheel: bool = "bShownOnWheel",
    }
}

meta_record! {
    /// `CWeaponComponentFlashLightInfo`, as the install's weapon metadata states it; carried unread: AccuracyModifier, DamageModifier, MainLightColor, MainLightVolumeOuterColor, SecondaryLightColor, SecondaryLightVolumeOuterColor.
    CWeaponComponentFlashLightInfo = "CWeaponComponentFlashLightInfo" {
        attach_bone: String = "AttachBone",
        create_object: bool = "CreateObject",
        flash_light_bone: String = "FlashLightBone",
        flash_light_bone_bulb_off: String = "FlashLightBoneBulbOff",
        flash_light_bone_bulb_on: String = "FlashLightBoneBulbOn",
        hud_accuracy: i32 = "HudAccuracy",
        hud_capacity: i32 = "HudCapacity",
        hud_damage: i32 = "HudDamage",
        hud_range: i32 = "HudRange",
        hud_speed: i32 = "HudSpeed",
        loc_desc: String = "LocDesc",
        loc_name: String = "LocName",
        main_light_corona_intensity: f32 = "MainLightCoronaIntensity",
        main_light_corona_size: f32 = "MainLightCoronaSize",
        main_light_falloff_exponent: f32 = "MainLightFalloffExponent",
        main_light_inner_angle: f32 = "MainLightInnerAngle",
        main_light_intensity: f32 = "MainLightIntensity",
        main_light_outer_angle: f32 = "MainLightOuterAngle",
        main_light_range: f32 = "MainLightRange",
        main_light_shadow_fade_distance: f32 = "MainLightShadowFadeDistance",
        main_light_specular_fade_distance: f32 = "MainLightSpecularFadeDistance",
        main_light_volume_exponent: f32 = "MainLightVolumeExponent",
        main_light_volume_intensity: f32 = "MainLightVolumeIntensity",
        main_light_volume_size: f32 = "MainLightVolumeSize",
        model: String = "Model",
        name: String = "Name",
        secondary_light_fade_distance: f32 = "SecondaryLightFadeDistance",
        secondary_light_falloff_exponent: f32 = "SecondaryLightFalloffExponent",
        secondary_light_inner_angle: f32 = "SecondaryLightInnerAngle",
        secondary_light_intensity: f32 = "SecondaryLightIntensity",
        secondary_light_outer_angle: f32 = "SecondaryLightOuterAngle",
        secondary_light_range: f32 = "SecondaryLightRange",
        secondary_light_volume_exponent: f32 = "SecondaryLightVolumeExponent",
        secondary_light_volume_intensity: f32 = "SecondaryLightVolumeIntensity",
        secondary_light_volume_size: f32 = "SecondaryLightVolumeSize",
        toggle_when_aiming: bool = "ToggleWhenAiming",
        b_shown_on_wheel: bool = "bShownOnWheel",
        f_target_distalong_aim_camera: f32 = "fTargetDistalongAimCamera",
    }
}

meta_record! {
    /// `CWeaponComponentScopeInfo`, as the install's weapon metadata states it; carried unread: AccuracyModifier, DamageModifier.
    CWeaponComponentScopeInfo = "CWeaponComponentScopeInfo" {
        attach_bone: String = "AttachBone",
        camera_hash: String = "CameraHash",
        create_object: bool = "CreateObject",
        extra_zoom_factor_for_accurate_mode: f32 = "ExtraZoomFactorForAccurateMode",
        hud_accuracy: i32 = "HudAccuracy",
        hud_capacity: i32 = "HudCapacity",
        hud_damage: i32 = "HudDamage",
        hud_range: i32 = "HudRange",
        hud_speed: i32 = "HudSpeed",
        loc_desc: String = "LocDesc",
        loc_name: String = "LocName",
        model: String = "Model",
        name: String = "Name",
        recoil_shake_amplitude: f32 = "RecoilShakeAmplitude",
        reticule: i32 = "Reticule",
        reticule_hash: String = "ReticuleHash",
        special_scope_type: String = "SpecialScopeType",
        b_shown_on_wheel: bool = "bShownOnWheel",
    }
}

meta_record! {
    /// `CWeaponComponentSuppressorInfo`, as the install's weapon metadata states it; carried unread: AccuracyModifier, DamageModifier.
    CWeaponComponentSuppressorInfo = "CWeaponComponentSuppressorInfo" {
        apply_weapon_tint: bool = "ApplyWeaponTint",
        attach_bone: String = "AttachBone",
        create_object: bool = "CreateObject",
        flash_fx: String = "FlashFX",
        flash_fx_: String = "FlashFx",
        hud_accuracy: i32 = "HudAccuracy",
        hud_capacity: i32 = "HudCapacity",
        hud_damage: i32 = "HudDamage",
        hud_range: i32 = "HudRange",
        hud_speed: i32 = "HudSpeed",
        loc_desc: String = "LocDesc",
        loc_name: String = "LocName",
        model: String = "Model",
        muzzle_bone: String = "MuzzleBone",
        name: String = "Name",
        recoil_shake_amplitude_modifier: f32 = "RecoilShakeAmplitudeModifier",
        should_silence: bool = "ShouldSilence",
        b_shown_on_wheel: bool = "bShownOnWheel",
    }
}

meta_record! {
    /// `CWeaponComponentVariantModelInfo`, as the install's weapon metadata states it; carried unread: AccuracyModifier, DamageModifier, ExtraComponents.
    CWeaponComponentVariantModelInfo = "CWeaponComponentVariantModelInfo" {
        attach_bone: String = "AttachBone",
        create_object: bool = "CreateObject",
        hud_accuracy: i32 = "HudAccuracy",
        hud_capacity: i32 = "HudCapacity",
        hud_damage: i32 = "HudDamage",
        hud_range: i32 = "HudRange",
        hud_speed: i32 = "HudSpeed",
        loc_desc: String = "LocDesc",
        loc_name: String = "LocName",
        model: String = "Model",
        name: String = "Name",
        tint_index_override: i32 = "TintIndexOverride",
        b_shown_on_wheel: bool = "bShownOnWheel",
    }
}

meta_origin!(CWeaponComponentInfoBlob);

impl File for CWeaponComponentInfoBlob {
    const EXTENSION: &'static str = "meta";

    fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let (mut root, arrival) = open_text("CWeaponComponentInfoBlob", bytes, Self::read)?;
        root.carry.arrival = Some(Box::new(arrival));
        Ok(root)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        save_text(self.write())
    }
}
