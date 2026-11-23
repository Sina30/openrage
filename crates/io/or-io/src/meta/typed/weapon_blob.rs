//! The root of a weapon file (`CWeaponInfoBlob`), the lists it keeps beside its infos and the infos themselves.

use super::weapon::*;
use super::*;

meta_record! {
    /// `CWeaponInfoBlob`, as the install's weapon metadata states it.
    CWeaponInfoBlob = "CWeaponInfoBlob" {
        aiming_infos: Vec<CAimingInfo> = "AimingInfos",
        firing_pattern_aliases: Vec<CWeaponFiringPatternAliases> = "FiringPatternAliases",
        infos: Vec<CWeaponInfoBlobInfos> = "Infos",
        name: String = "Name",
        slot_best_order: CWeaponInfoBlobSWeaponSlotList = "SlotBestOrder",
        slot_navigate_order: Vec<CWeaponInfoBlobSWeaponSlotList> = "SlotNavigateOrder",
        tint_spec_values: Vec<CWeaponTintSpecValues> = "TintSpecValues",
        upper_body_fixup_expression_data: Vec<CWeaponUpperBodyFixupExpressionData> = "UpperBodyFixupExpressionData",
        vehicle_weapon_infos: Vec<CVehicleWeaponInfo> = "VehicleWeaponInfos",
        weapon_group_damage_for_armoured_vehicle_glass: Vec<CWeaponInfoBlobSWeaponGroupArmouredGlassDamage> = "WeaponGroupDamageForArmouredVehicleGlass",
    }
}

meta_record! {
    /// `CWeaponInfoBlob::sWeaponSlotList`, as the install's weapon metadata states it.
    CWeaponInfoBlobSWeaponSlotList = "CWeaponInfoBlob__sWeaponSlotList" {
        weapon_slots: Vec<CWeaponInfoBlobSlotEntry> = "WeaponSlots",
    }
}

meta_record! {
    /// `CWeaponInfoBlob::SlotEntry`, as the install's weapon metadata states it.
    CWeaponInfoBlobSlotEntry = "CWeaponInfoBlob__SlotEntry" {
        entry: String = "Entry",
        order_number: i32 = "OrderNumber",
    }
}

meta_record! {
    /// `CWeaponTintSpecValues`, as the install's weapon metadata states it.
    CWeaponTintSpecValues = "CWeaponTintSpecValues" {
        name: String = "Name",
        tints: Vec<CWeaponTintSpecValuesCWeaponSpecValue> = "Tints",
    }
}

meta_record! {
    /// `CWeaponTintSpecValues::CWeaponSpecValue`, as the install's weapon metadata states it; carried unread: Spec2Color.
    CWeaponTintSpecValuesCWeaponSpecValue = "CWeaponTintSpecValues__CWeaponSpecValue" {
        spec2_color_int: f32 = "Spec2ColorInt",
        spec2_factor: f32 = "Spec2Factor",
        spec_falloff_mult: f32 = "SpecFalloffMult",
        spec_fresnel: f32 = "SpecFresnel",
        spec_int_mult: f32 = "SpecIntMult",
    }
}

meta_record! {
    /// `CWeaponFiringPatternAliases`, as the install's weapon metadata states it.
    CWeaponFiringPatternAliases = "CWeaponFiringPatternAliases" {
        aliases: Vec<CWeaponFiringPatternAliasesCFiringPatternAlias> = "Aliases",
        name: String = "Name",
    }
}

meta_record! {
    /// `CWeaponFiringPatternAliases::CFiringPatternAlias`, as the install's weapon metadata states it.
    CWeaponFiringPatternAliasesCFiringPatternAlias = "CWeaponFiringPatternAliases__CFiringPatternAlias" {
        alias: String = "Alias",
        firing_pattern: String = "FiringPattern",
    }
}

meta_record! {
    /// `CWeaponUpperBodyFixupExpressionData`, as the install's weapon metadata states it.
    CWeaponUpperBodyFixupExpressionData = "CWeaponUpperBodyFixupExpressionData" {
        data: Vec<CWeaponUpperBodyFixupExpressionDataData> = "Data",
        name: String = "Name",
    }
}

meta_record! {
    /// `CWeaponUpperBodyFixupExpressionData::Data`, as the install's weapon metadata states it.
    CWeaponUpperBodyFixupExpressionDataData = "CWeaponUpperBodyFixupExpressionData__Data" {
        idle: f32 = "Idle",
        run: f32 = "Run",
        walk: f32 = "Walk",
    }
}

meta_record! {
    /// `CWeaponInfoBlob::sWeaponGroupArmouredGlassDamage`, as the install's weapon metadata states it.
    CWeaponInfoBlobSWeaponGroupArmouredGlassDamage = "CWeaponInfoBlob__sWeaponGroupArmouredGlassDamage" {
        damage: f32 = "Damage",
        group_hash: String = "GroupHash",
    }
}

meta_record! {
    /// One element of the root's `Infos`: the install's XML states no type for it, so the name is this declaration's own and no file carries it.
    CWeaponInfoBlobInfos = "CWeaponInfoBlob__Infos" {
        infos: Vec<InfoAny> = "Infos",
    }
}

meta_origin!(CWeaponInfoBlob);

impl File for CWeaponInfoBlob {
    const EXTENSION: &'static str = "meta";

    fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let (mut root, arrival) = open_text("CWeaponInfoBlob", bytes, Self::read)?;
        root.carry.arrival = Some(Box::new(arrival));
        Ok(root)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        save_text(self.write())
    }
}
