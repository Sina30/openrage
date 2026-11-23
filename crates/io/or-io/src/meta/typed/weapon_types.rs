//! The weapon family's types, declared here so a weapon document is read, built and saved where no schema snapshot is installed, which is every user's machine.
//!
//! Written in the PSC's dialect from the XML of the weapon metadata the install ships: a member is declared when
//! every element of it in the install has one shape, and a member whose shapes differ (a name in one file and a number in
//! another), a reference to a named item (`ref`) and a null pointer stay out, and a document carries them as it arrived.
//! `every_weapon_file_of_the_install_opens_saves_and_arrives_as_it_was` holds each class and member to the documents.
//!
//! ⚠ **A name in a document is a type of the family only where the document says so**: a pointer states its target, and
//! the engine's name for the unnamed element of `Infos` is stated nowhere in the XML, so that one type is named for what it is
//! (`CWeaponInfoBlob__Infos`) and is never written to a file.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::OnceLock;

use crate::meta::binary::synth::name_hash;
use crate::meta::schema::{parse_psc, SchemaItem, SchemaLibrary};

pub const WEAPON_TYPES: &str = r#"<ParserSchema>
  <structdef type="CWeaponInfoBlob">
    <array name="AimingInfos" type="atArray"><pointer type="CAimingInfo" policy="owner"/></array>
    <array name="FiringPatternAliases" type="atArray"><struct type="CWeaponFiringPatternAliases"/></array>
    <array name="Infos" type="atArray"><struct type="CWeaponInfoBlob__Infos"/></array>
    <string name="Name" type="atString"/>
    <struct name="SlotBestOrder" type="CWeaponInfoBlob__sWeaponSlotList"/>
    <array name="SlotNavigateOrder" type="atArray"><struct type="CWeaponInfoBlob__sWeaponSlotList"/></array>
    <array name="TintSpecValues" type="atArray"><struct type="CWeaponTintSpecValues"/></array>
    <array name="UpperBodyFixupExpressionData" type="atArray"><struct type="CWeaponUpperBodyFixupExpressionData"/></array>
    <array name="VehicleWeaponInfos" type="atArray"><pointer type="CVehicleWeaponInfo" policy="owner"/></array>
    <array name="WeaponGroupDamageForArmouredVehicleGlass" type="atArray"><struct type="CWeaponInfoBlob__sWeaponGroupArmouredGlassDamage"/></array>
  </structdef>
  <structdef type="CWeaponInfoBlob__sWeaponSlotList">
    <array name="WeaponSlots" type="atArray"><struct type="CWeaponInfoBlob__SlotEntry"/></array>
  </structdef>
  <structdef type="CWeaponInfoBlob__SlotEntry">
    <string name="Entry" type="atString"/>
    <s32 name="OrderNumber"/>
  </structdef>
  <structdef type="CWeaponTintSpecValues">
    <string name="Name" type="atString"/>
    <array name="Tints" type="atArray"><struct type="CWeaponTintSpecValues__CWeaponSpecValue"/></array>
  </structdef>
  <structdef type="CWeaponTintSpecValues__CWeaponSpecValue">
    <float name="Spec2ColorInt"/>
    <float name="Spec2Factor"/>
    <float name="SpecFalloffMult"/>
    <float name="SpecFresnel"/>
    <float name="SpecIntMult"/>
  </structdef>
  <structdef type="CWeaponFiringPatternAliases">
    <array name="Aliases" type="atArray"><struct type="CWeaponFiringPatternAliases__CFiringPatternAlias"/></array>
    <string name="Name" type="atString"/>
  </structdef>
  <structdef type="CWeaponFiringPatternAliases__CFiringPatternAlias">
    <string name="Alias" type="atString"/>
    <string name="FiringPattern" type="atString"/>
  </structdef>
  <structdef type="CWeaponUpperBodyFixupExpressionData">
    <array name="Data" type="atArray"><struct type="CWeaponUpperBodyFixupExpressionData__Data"/></array>
    <string name="Name" type="atString"/>
  </structdef>
  <structdef type="CWeaponUpperBodyFixupExpressionData__Data">
    <float name="Idle"/>
    <float name="Run"/>
    <float name="Walk"/>
  </structdef>
  <structdef type="CAimingInfo">
    <float name="HeadingLimit"/>
    <string name="Name" type="atString"/>
    <float name="SweepPitchMax"/>
    <float name="SweepPitchMin"/>
  </structdef>
  <structdef type="CVehicleWeaponInfo">
    <float name="KickbackAmplitude"/>
    <float name="KickbackImpulse"/>
    <float name="KickbackOverrideTiming"/>
    <string name="Name" type="atString"/>
  </structdef>
  <structdef type="CWeaponInfoBlob__sWeaponGroupArmouredGlassDamage">
    <float name="Damage"/>
    <string name="GroupHash" type="atString"/>
  </structdef>
  <structdef type="CWeaponInfoBlob__Infos">
    <array name="Infos" type="atArray"><pointer type="CItemInfo" policy="owner"/></array>
  </structdef>
  <structdef type="CWeaponInfo">
    <string name="AccuracyOffsetShakeHash" type="atString"/>
    <float name="AccuracySpread"/>
    <float name="AccurateModeAccuracyModifier"/>
    <float name="AiPotentialBlastEventRange"/>
    <float name="AiSoundRange"/>
    <string name="AimCameraHash" type="atString"/>
    <Vector3 name="AimOffsetEndPosMaxFPSIdle"/>
    <Vector3 name="AimOffsetEndPosMaxFPSLT"/>
    <Vector3 name="AimOffsetEndPosMedFPSIdle"/>
    <Vector3 name="AimOffsetEndPosMedFPSLT"/>
    <Vector3 name="AimOffsetEndPosMinFPSIdle"/>
    <Vector3 name="AimOffsetEndPosMinFPSLT"/>
    <Vector3 name="AimOffsetMax"/>
    <Vector3 name="AimOffsetMaxFPSIdle"/>
    <Vector3 name="AimOffsetMaxFPSLT"/>
    <Vector3 name="AimOffsetMaxFPSRNG"/>
    <Vector3 name="AimOffsetMaxFPSScope"/>
    <Vector3 name="AimOffsetMedFPSIdle"/>
    <Vector3 name="AimOffsetMin"/>
    <Vector3 name="AimOffsetMinFPSIdle"/>
    <Vector3 name="AimOffsetMinFPSLT"/>
    <Vector3 name="AimOffsetMinFPSRNG"/>
    <Vector3 name="AimOffsetMinFPSScope"/>
    <float name="AimProbeLengthMax"/>
    <float name="AimProbeLengthMin"/>
    <float name="AimProbeRadiusOverrideFPSIdle"/>
    <float name="AimProbeRadiusOverrideFPSIdleStealth"/>
    <float name="AimProbeRadiusOverrideFPSLT"/>
    <float name="AimProbeRadiusOverrideFPSRNG"/>
    <float name="AimProbeRadiusOverrideFPSScope"/>
    <float name="AimingBreathingAdditiveWeight"/>
    <float name="AimingLeanAdditiveWeight"/>
    <float name="AirborneAircraftLockOnMultiplier"/>
    <float name="AlternateWaitTime"/>
    <string name="AlternativeOrScopedCameraHash" type="atString"/>
    <s32 name="AmmoDiminishingRate"/>
    <float name="AnimReloadRate"/>
    <float name="ArmouredVehicleGlassDamageOverride"/>
    <string name="AssistedAimingCameraHash" type="atString"/>
    <string name="AssistedAlternativeOrScopedCameraHash" type="atString"/>
    <array name="AttachPoints" type="atArray"><struct type="CWeaponComponentPoint"/></array>
    <string name="Audio" type="atString"/>
    <string name="AudioCollisionHash" type="atString"/>
    <float name="BatchSpread"/>
    <float name="BulletBendingFarRadius"/>
    <float name="BulletBendingNearRadius"/>
    <float name="BulletBendingZoomedRadius"/>
    <float name="BulletDirectionOffsetInDegrees"/>
    <float name="BulletDirectionPitchHomingOffset"/>
    <float name="BulletDirectionPitchOffset"/>
    <s32 name="BulletsInBatch"/>
    <s32 name="BulletsPerAnimLoop"/>
    <float name="CameraFov"/>
    <string name="CinematicShootingAlternativeOrScopedCameraHash" type="atString"/>
    <string name="CinematicShootingCameraHash" type="atString"/>
    <s32 name="ClipSize"/>
    <string name="CoverCameraHash" type="atString"/>
    <string name="CoverReadyToFireCameraHash" type="atString"/>
    <float name="Damage"/>
    <float name="DamageFallOffModifier"/>
    <float name="DamageFallOffRangeMax"/>
    <float name="DamageFallOffRangeMin"/>
    <float name="DamageTime"/>
    <float name="DamageTimeInVehicle"/>
    <float name="DamageTimeInVehicleHeadShot"/>
    <string name="DamageType" type="atString"/>
    <string name="DefaultCameraHash" type="atString"/>
    <float name="DropForwardVelocity"/>
    <float name="EnduranceDamage"/>
    <float name="ExpandPedCapsuleRadius"/>
    <float name="ExplosionShakeAmplitude"/>
    <string name="FireCameraHash" type="atString"/>
    <string name="FireType" type="atString"/>
    <float name="FiringBreathingAdditiveWeight"/>
    <float name="FiringLeanAdditiveWeight"/>
    <float name="FirstPersonAimFovMax"/>
    <float name="FirstPersonAimFovMin"/>
    <Vector3 name="FirstPersonAsThirdPersonIdleOffset"/>
    <Vector3 name="FirstPersonAsThirdPersonLTOffset"/>
    <Vector3 name="FirstPersonAsThirdPersonRNGOffset"/>
    <Vector3 name="FirstPersonAsThirdPersonScopeOffset"/>
    <Vector3 name="FirstPersonAsThirdPersonWeaponBlockedOffset"/>
    <float name="FirstPersonBulletBendingFarRadius"/>
    <float name="FirstPersonBulletBendingNearRadius"/>
    <float name="FirstPersonBulletBendingZoomedRadius"/>
    <float name="FirstPersonDofMaxNearInFocusDistance"/>
    <float name="FirstPersonDofMaxNearInFocusDistanceBlendLevel"/>
    <float name="FirstPersonDofSubjectMagnificationPowerFactorNear"/>
    <Vector3 name="FirstPersonDrivebyIKOffset"/>
    <Vector3 name="FirstPersonLTOffset"/>
    <Vector3 name="FirstPersonLTRotationOffset"/>
    <Vector3 name="FirstPersonRNGOffset"/>
    <Vector3 name="FirstPersonRNGRotationOffset"/>
    <string name="FirstPersonReticuleStyleHash" type="atString"/>
    <array name="FirstPersonScopeAttachmentData" type="atArray"><struct type="CWeaponInfo__sFirstPersonScopeAttachmentData"/></array>
    <float name="FirstPersonScopeAttachmentFov"/>
    <Vector3 name="FirstPersonScopeAttachmentOffset"/>
    <Vector3 name="FirstPersonScopeAttachmentRotationOffset"/>
    <float name="FirstPersonScopeFov"/>
    <Vector3 name="FirstPersonScopeOffset"/>
    <Vector3 name="FirstPersonScopeRotationOffset"/>
    <float name="Force"/>
    <float name="ForceFalloffMin"/>
    <float name="ForceFalloffRangeEnd"/>
    <float name="ForceFalloffRangeStart"/>
    <float name="ForceHitFlyingHeli"/>
    <float name="ForceHitPed"/>
    <float name="ForceHitVehicle"/>
    <float name="ForceMaxStrengthMult"/>
    <float name="FragImpulse"/>
    <struct name="FrontClearTestParams" type="CWeaponInfo__sFrontClearTestParams"/>
    <struct name="Fx" type="CWeaponInfo__sFx"/>
    <string name="Group" type="atString"/>
    <string name="GunFeedBone" type="atString"/>
    <float name="HeadShotDamageModifierAI"/>
    <float name="HeadShotDamageModifierPlayer"/>
    <float name="HitLimbsDamageModifier"/>
    <s32 name="HudAccuracy"/>
    <s32 name="HudCapacity"/>
    <s32 name="HudDamage"/>
    <s32 name="HudRange"/>
    <s32 name="HudSpeed"/>
    <string name="HumanNameHash" type="atString"/>
    <float name="IkRecoilDisplacement"/>
    <float name="IkRecoilDisplacementScaleBackward"/>
    <float name="IkRecoilDisplacementScaleVertical"/>
    <float name="IkRecoilDisplacementScope"/>
    <s32 name="InitialRumbleDuration"/>
    <s32 name="InitialRumbleDurationFps"/>
    <float name="InitialRumbleIntensity"/>
    <float name="InitialRumbleIntensityFps"/>
    <float name="InitialRumbleIntensityTrigger"/>
    <float name="KillshotImpulseScale"/>
    <s32 name="KnockdownCount"/>
    <Vector3 name="LeftHandIkOffset"/>
    <float name="LightlyArmouredDamageModifier"/>
    <float name="LockOnRange"/>
    <string name="MPPickupHash" type="atString"/>
    <float name="MaxHeadShotDistanceAI"/>
    <float name="MaxHeadShotDistancePlayer"/>
    <float name="MeleeDamageMultiplier"/>
    <float name="MeleeRightFistTargetHealthDamageScaler"/>
    <float name="MinHeadShotDistanceAI"/>
    <float name="MinHeadShotDistancePlayer"/>
    <s32 name="MinTimeBetweenRecoilShakes"/>
    <string name="Model" type="atString"/>
    <string name="MovementModeConditionalIdle" type="atString"/>
    <string name="Name" type="atString"/>
    <float name="NetworkHeadShotPlayerDamageModifier"/>
    <float name="NetworkHitLimbsDamageModifier"/>
    <float name="NetworkPedDamageModifier"/>
    <float name="NetworkPlayerDamageModifier"/>
    <string name="NmShotTuningSet" type="atString"/>
    <float name="Penetration"/>
    <string name="PickupHash" type="atString"/>
    <string name="PlayerDamageOverTimeWeapon" type="atString"/>
    <string name="PovTurretCameraHash" type="atString"/>
    <float name="ProjectileForce"/>
    <float name="RecoilAccuracyMax"/>
    <float name="RecoilAccuracyToAllowHeadShotAI"/>
    <float name="RecoilAccuracyToAllowHeadShotPlayer"/>
    <float name="RecoilErrorTime"/>
    <float name="RecoilRecoveryRate"/>
    <float name="RecoilShakeAmplitude"/>
    <string name="RecoilShakeHash" type="atString"/>
    <string name="RecoilShakeHashFirstPerson" type="atString"/>
    <float name="ReloadTimeMP"/>
    <float name="ReloadTimeSP"/>
    <Vector2 name="ReticuleHudPosition"/>
    <Vector2 name="ReticuleHudPositionOffsetForPOVTurret"/>
    <float name="ReticuleMinSizeCrouched"/>
    <float name="ReticuleMinSizeStanding"/>
    <float name="ReticuleScale"/>
    <string name="ReticuleStyleHash" type="atString"/>
    <string name="RotateBarrelBone" type="atString"/>
    <string name="RotateBarrelBone2" type="atString"/>
    <float name="RumbleDamageIntensity"/>
    <s32 name="RumbleDuration"/>
    <s32 name="RumbleDurationFps"/>
    <float name="RumbleIntensity"/>
    <float name="RumbleIntensityFps"/>
    <float name="RumbleIntensityTrigger"/>
    <float name="RunAndGunAccuracyMaxModifier"/>
    <float name="RunAndGunAccuracyMinOverride"/>
    <float name="RunAndGunAccuracyModifier"/>
    <string name="RunAndGunAlternativeOrScopedCameraHash" type="atString"/>
    <string name="RunAndGunCameraHash" type="atString"/>
    <string name="Slot" type="atString"/>
    <float name="Speed"/>
    <float name="SpinDownTime"/>
    <float name="SpinTime"/>
    <float name="SpinUpTime"/>
    <string name="StatName" type="atString"/>
    <float name="StealthAimingBreathingAdditiveWeight"/>
    <float name="StealthAimingLeanAdditiveWeight"/>
    <float name="StealthFiringBreathingAdditiveWeight"/>
    <float name="StealthFiringLeanAdditiveWeight"/>
    <string name="TargetSequenceGroup" type="atString"/>
    <float name="TimeBetweenShots"/>
    <float name="TimeLeftBetweenShotsWhereShouldFireIsCached"/>
    <Vector2 name="TorsoAimOffset"/>
    <Vector2 name="TorsoCrouchedAimOffset"/>
    <float name="TorsoIKAngleLimit"/>
    <float name="VehicleAttackAngle"/>
    <float name="VehicleDamageModifier"/>
    <float name="VehicleReloadTime"/>
    <string name="VehicleWeaponHash" type="atString"/>
    <float name="VerticalLaunchAdjustment"/>
    <string name="WeaponFlags" type="atString"/>
    <float name="WeaponRange"/>
    <string name="WheelSlot" type="atString"/>
    <float name="ZoomFactorForAccurateMode"/>
  </structdef>
  <structdef type="CWeaponComponentPoint">
    <string name="AttachBone" type="atString"/>
    <array name="Components" type="atArray"><struct type="CWeaponComponentPoint__sComponent"/></array>
  </structdef>
  <structdef type="CWeaponComponentPoint__sComponent">
    <bool name="Default"/>
    <string name="Name" type="atString"/>
  </structdef>
  <structdef type="CWeaponInfo__sFx">
    <string name="EffectGroup" type="atString"/>
    <string name="FlashFx" type="atString"/>
    <string name="FlashFxAlt" type="atString"/>
    <float name="FlashFxAltChance"/>
    <string name="FlashFxAltFP" type="atString"/>
    <float name="FlashFxChanceMP"/>
    <float name="FlashFxChanceSP"/>
    <string name="FlashFxEmptyShot" type="atString"/>
    <string name="FlashFxFP" type="atString"/>
    <string name="FlashFxFPAlt" type="atString"/>
    <bool name="FlashFxLightCastsShadows"/>
    <bool name="FlashFxLightEnabled"/>
    <Vector2 name="FlashFxLightFalloffMinMax"/>
    <Vector2 name="FlashFxLightIntensityMinMax"/>
    <float name="FlashFxLightOffsetDist"/>
    <Vector3 name="FlashFxLightRGBAMax"/>
    <Vector3 name="FlashFxLightRGBAMin"/>
    <float name="FlashFxScale"/>
    <float name="GroundDisturbFxDist"/>
    <bool name="GroundDisturbFxEnabled"/>
    <string name="GroundDisturbFxNameDefault" type="atString"/>
    <string name="GroundDisturbFxNameDirt" type="atString"/>
    <string name="GroundDisturbFxNameFoliage" type="atString"/>
    <string name="GroundDisturbFxNameSand" type="atString"/>
    <string name="GroundDisturbFxNameWater" type="atString"/>
    <Vector3 name="MuzzleOverrideOffset"/>
    <string name="MuzzleSmokeFx" type="atString"/>
    <float name="MuzzleSmokeFxDecPerSec"/>
    <string name="MuzzleSmokeFxFP" type="atString"/>
    <float name="MuzzleSmokeFxIncPerShot"/>
    <float name="MuzzleSmokeFxMinLevel"/>
    <bool name="MuzzleUseProjTintColour"/>
    <string name="PedDamageHash" type="atString"/>
    <string name="ShellFx" type="atString"/>
    <string name="ShellFxFP" type="atString"/>
    <string name="TracerFx" type="atString"/>
    <float name="TracerFxChanceMP"/>
    <float name="TracerFxChanceSP"/>
    <bool name="TracerFxIgnoreCameraIntersection"/>
  </structdef>
  <structdef type="CWeaponInfo__sFrontClearTestParams">
    <float name="CapsuleLength"/>
    <float name="CapsuleRadius"/>
    <float name="ForwardOffset"/>
    <float name="HorizontalOffset"/>
    <bool name="ShouldPerformFrontClearTest"/>
    <float name="VerticalOffset"/>
  </structdef>
  <structdef type="CWeaponInfo__sFirstPersonScopeAttachmentData">
    <float name="FirstPersonScopeAttachmentFov"/>
    <Vector3 name="FirstPersonScopeAttachmentOffset"/>
    <Vector3 name="FirstPersonScopeAttachmentRotationOffset"/>
    <string name="Name" type="atString"/>
  </structdef>
  <structdef type="CAmmoInfo">
    <string name="AmmoFlags" type="atString"/>
    <s32 name="AmmoMax"/>
    <s32 name="AmmoMax100"/>
    <s32 name="AmmoMax100MP"/>
    <s32 name="AmmoMax50"/>
    <s32 name="AmmoMax50MP"/>
    <s32 name="AmmoMaxMP"/>
    <string name="AmmoSpecialType" type="atString"/>
    <string name="Audio" type="atString"/>
    <string name="Model" type="atString"/>
    <string name="Name" type="atString"/>
    <string name="Slot" type="atString"/>
  </structdef>
  <structdef type="CAmmoProjectileInfo">
    <string name="AmmoFlags" type="atString"/>
    <s32 name="AmmoMax"/>
    <s32 name="AmmoMax100"/>
    <s32 name="AmmoMax100MP"/>
    <s32 name="AmmoMax50"/>
    <s32 name="AmmoMax50MP"/>
    <s32 name="AmmoMaxMP"/>
    <string name="AmmoSpecialType" type="atString"/>
    <string name="Audio" type="atString"/>
    <bool name="ChargedLaunchBlockedDuringReload"/>
    <float name="ChargedLaunchSpeedMult"/>
    <float name="ChargedLaunchTime"/>
    <s32 name="ClusterExplosionCount"/>
    <string name="ClusterExplosionTag" type="atString"/>
    <float name="ClusterInbetweenDelay"/>
    <float name="ClusterInitialDelay"/>
    <float name="ClusterMaxRadius"/>
    <float name="ClusterMinRadius"/>
    <float name="CoronaIntensity"/>
    <float name="CoronaSize"/>
    <float name="CoronaZBias"/>
    <float name="Damage"/>
    <float name="Damping"/>
    <string name="DisturbFxDefault" type="atString"/>
    <string name="DisturbFxDirt" type="atString"/>
    <string name="DisturbFxFoliage" type="atString"/>
    <float name="DisturbFxProbeDist"/>
    <string name="DisturbFxSand" type="atString"/>
    <float name="DisturbFxScale"/>
    <string name="DisturbFxWater" type="atString"/>
    <struct name="Explosion" type="CAmmoProjectileInfo__sExplosion"/>
    <float name="ExplosionTime"/>
    <float name="FrictionMultiplier"/>
    <float name="FromVehicleLifeTime"/>
    <string name="FuseFx" type="atString"/>
    <string name="FuseFxFP" type="atString"/>
    <bool name="FxAltTintColour"/>
    <float name="GravityFactor"/>
    <float name="GroundFxProbeDistance"/>
    <float name="LaunchSpeed"/>
    <float name="LifeTime"/>
    <float name="LifeTimeAfterExplosion"/>
    <float name="LifeTimeAfterImpact"/>
    <string name="LightBone" type="atString"/>
    <Vector3 name="LightColour"/>
    <float name="LightFalloffExp"/>
    <bool name="LightFlickers"/>
    <float name="LightFrequency"/>
    <float name="LightIntensity"/>
    <bool name="LightOnlyActiveWhenStuck"/>
    <float name="LightPower"/>
    <float name="LightRange"/>
    <bool name="LightSpeedsUp"/>
    <string name="Model" type="atString"/>
    <string name="Name" type="atString"/>
    <float name="PedRicochetTolerance"/>
    <string name="PrimedFx" type="atString"/>
    <string name="PrimedFxFP" type="atString"/>
    <string name="ProjectileFlags" type="atString"/>
    <float name="ProximityActivationTime"/>
    <float name="ProximityFuseTimePed"/>
    <float name="ProximityFuseTimeVehicleMax"/>
    <float name="ProximityFuseTimeVehicleMin"/>
    <float name="ProximityFuseTimeVehicleSpeed"/>
    <string name="ProximityFx" type="atString"/>
    <Vector3 name="ProximityLightColourUntriggered"/>
    <float name="ProximityLightFrequencyMultiplierTriggered"/>
    <float name="ProximityTriggerRadius"/>
    <float name="RicochetTolerance"/>
    <float name="SeparationTime"/>
    <string name="Slot" type="atString"/>
    <float name="TimeToIgnoreOwner"/>
    <float name="TimeToReachTarget"/>
    <string name="TrailFx" type="atString"/>
    <float name="TrailFxFadeInTime"/>
    <float name="TrailFxFadeOutTime"/>
    <string name="TrailFxUnderWater" type="atString"/>
    <float name="VehicleRicochetTolerance"/>
  </structdef>
  <structdef type="CAmmoProjectileInfo__sExplosion">
    <string name="Default" type="atString"/>
    <string name="HitBike" type="atString"/>
    <string name="HitBoat" type="atString"/>
    <string name="HitCar" type="atString"/>
    <string name="HitPlane" type="atString"/>
    <string name="HitTruck" type="atString"/>
  </structdef>
  <structdef type="CAmmoRocketInfo">
    <string name="AmmoFlags" type="atString"/>
    <s32 name="AmmoMax"/>
    <s32 name="AmmoMax100"/>
    <s32 name="AmmoMax100MP"/>
    <s32 name="AmmoMax50"/>
    <s32 name="AmmoMax50MP"/>
    <s32 name="AmmoMaxMP"/>
    <string name="AmmoSpecialType" type="atString"/>
    <string name="Audio" type="atString"/>
    <float name="ChargedLaunchSpeedMult"/>
    <float name="ChargedLaunchTime"/>
    <s32 name="ClusterExplosionCount"/>
    <string name="ClusterExplosionTag" type="atString"/>
    <float name="ClusterInbetweenDelay"/>
    <float name="ClusterInitialDelay"/>
    <float name="ClusterMaxRadius"/>
    <float name="ClusterMinRadius"/>
    <float name="CoronaIntensity"/>
    <float name="CoronaSize"/>
    <float name="CoronaZBias"/>
    <float name="Damage"/>
    <float name="Damping"/>
    <string name="DisturbFxDefault" type="atString"/>
    <string name="DisturbFxDirt" type="atString"/>
    <string name="DisturbFxFoliage" type="atString"/>
    <float name="DisturbFxProbeDist"/>
    <string name="DisturbFxSand" type="atString"/>
    <float name="DisturbFxScale"/>
    <string name="DisturbFxWater" type="atString"/>
    <struct name="Explosion" type="CAmmoProjectileInfo__sExplosion"/>
    <float name="ExplosionTime"/>
    <float name="ForwardDragCoeff"/>
    <float name="FrictionMultiplier"/>
    <float name="FromVehicleLifeTime"/>
    <string name="FuseFx" type="atString"/>
    <string name="FuseFxFP" type="atString"/>
    <bool name="FxAltTintColour"/>
    <float name="GravityFactor"/>
    <float name="GroundFxProbeDistance"/>
    <float name="LaunchSpeed"/>
    <float name="LifeTime"/>
    <float name="LifeTimeAfterExplosion"/>
    <float name="LifeTimeAfterImpact"/>
    <float name="LifeTimePlayerVehicleLockedOverrideMP"/>
    <string name="LightBone" type="atString"/>
    <Vector3 name="LightColour"/>
    <float name="LightFalloffExp"/>
    <bool name="LightFlickers"/>
    <float name="LightFrequency"/>
    <float name="LightIntensity"/>
    <bool name="LightOnlyActiveWhenStuck"/>
    <float name="LightPower"/>
    <float name="LightRange"/>
    <bool name="LightSpeedsUp"/>
    <float name="MaxRollAngleSin"/>
    <string name="Model" type="atString"/>
    <string name="Name" type="atString"/>
    <float name="PedRicochetTolerance"/>
    <float name="PitchChangeRate"/>
    <string name="PrimedFx" type="atString"/>
    <string name="PrimedFxFP" type="atString"/>
    <string name="ProjectileFlags" type="atString"/>
    <float name="ProximityActivationTime"/>
    <bool name="ProximityAffectsFiringPlayer"/>
    <bool name="ProximityCanBeTriggeredByPeds"/>
    <float name="ProximityFuseTimePed"/>
    <float name="ProximityFuseTimeVehicleMax"/>
    <float name="ProximityFuseTimeVehicleMin"/>
    <float name="ProximityFuseTimeVehicleSpeed"/>
    <string name="ProximityFx" type="atString"/>
    <Vector3 name="ProximityLightColourUntriggered"/>
    <float name="ProximityLightFrequencyMultiplierTriggered"/>
    <float name="ProximityRadius"/>
    <float name="ProximityRepeatedDetonationActivationTime"/>
    <float name="ProximityTriggerRadius"/>
    <float name="RicochetTolerance"/>
    <float name="RollChangeRate"/>
    <float name="SeparationTime"/>
    <float name="SideDragCoeff"/>
    <string name="Slot" type="atString"/>
    <float name="TimeBeforeHoming"/>
    <float name="TimeBeforeSwitchTargetMax"/>
    <float name="TimeBeforeSwitchTargetMin"/>
    <float name="TimeToIgnoreOwner"/>
    <float name="TimeToReachTarget"/>
    <string name="TrailFx" type="atString"/>
    <float name="TrailFxFadeInTime"/>
    <float name="TrailFxFadeOutTime"/>
    <string name="TrailFxUnderWater" type="atString"/>
    <Vector3 name="UntriggeredProximityLightColour"/>
    <float name="VehicleRicochetTolerance"/>
    <float name="YawChangeRate"/>
  </structdef>
  <structdef type="CAmmoThrownInfo">
    <string name="AmmoFlags" type="atString"/>
    <s32 name="AmmoMax"/>
    <s32 name="AmmoMax100"/>
    <s32 name="AmmoMax100MP"/>
    <s32 name="AmmoMax50"/>
    <s32 name="AmmoMax50MP"/>
    <s32 name="AmmoMaxMP"/>
    <s32 name="AmmoMaxMPBonus"/>
    <string name="Audio" type="atString"/>
    <float name="CoronaIntensity"/>
    <float name="CoronaSize"/>
    <float name="CoronaZBias"/>
    <float name="Damage"/>
    <float name="Damping"/>
    <string name="DisturbFxDefault" type="atString"/>
    <string name="DisturbFxDirt" type="atString"/>
    <string name="DisturbFxFoliage" type="atString"/>
    <float name="DisturbFxProbeDist"/>
    <string name="DisturbFxSand" type="atString"/>
    <float name="DisturbFxScale"/>
    <string name="DisturbFxWater" type="atString"/>
    <struct name="Explosion" type="CAmmoProjectileInfo__sExplosion"/>
    <float name="ExplosionTime"/>
    <float name="FrictionMultiplier"/>
    <float name="FromVehicleLifeTime"/>
    <string name="FuseFx" type="atString"/>
    <string name="FuseFxFP" type="atString"/>
    <float name="GravityFactor"/>
    <float name="GroundFxProbeDistance"/>
    <float name="LaunchSpeed"/>
    <float name="LifeTime"/>
    <float name="LifeTimeAfterExplosion"/>
    <float name="LifeTimeAfterImpact"/>
    <string name="LightBone" type="atString"/>
    <Vector3 name="LightColour"/>
    <float name="LightFalloffExp"/>
    <bool name="LightFlickers"/>
    <float name="LightFrequency"/>
    <float name="LightIntensity"/>
    <bool name="LightOnlyActiveWhenStuck"/>
    <float name="LightPower"/>
    <float name="LightRange"/>
    <bool name="LightSpeedsUp"/>
    <string name="Model" type="atString"/>
    <string name="Name" type="atString"/>
    <float name="PedRicochetTolerance"/>
    <string name="PrimedFx" type="atString"/>
    <string name="PrimedFxFP" type="atString"/>
    <string name="ProjectileFlags" type="atString"/>
    <Vector3 name="ProximityLightColourUntriggered"/>
    <float name="RicochetTolerance"/>
    <float name="SeparationTime"/>
    <bool name="ShouldGenerateImpactVFX"/>
    <string name="Slot" type="atString"/>
    <float name="ThrownForce"/>
    <float name="ThrownForceFromVehicle"/>
    <float name="TimeToReachTarget"/>
    <string name="TrailFx" type="atString"/>
    <float name="TrailFxFadeInTime"/>
    <float name="TrailFxFadeOutTime"/>
    <string name="TrailFxUnderWater" type="atString"/>
    <Vector3 name="UntriggeredProximityLightColour"/>
    <float name="VehicleRicochetTolerance"/>
  </structdef>
  <structdef type="CWeaponComponentInfoBlob">
    <array name="Data" type="atArray"><pointer type="CWeaponComponentData" policy="owner"/></array>
    <string name="InfoBlobName" type="atString"/>
    <array name="Infos" type="atArray"><pointer type="CWeaponComponentInfo" policy="owner"/></array>
  </structdef>
  <structdef type="CWeaponComponentReloadData">
    <float name="AnimRateModifier"/>
    <string name="Name" type="atString"/>
    <string name="PedAimReloadClipId" type="atString"/>
    <string name="PedAimReloadEmptyClipId" type="atString"/>
    <string name="PedIdleReloadClipId" type="atString"/>
    <string name="PedIdleReloadEmptyClipId" type="atString"/>
    <string name="PedLowLeftCoverReloadClipId" type="atString"/>
    <string name="PedLowRightCoverReloadClipId" type="atString"/>
    <string name="WeaponAimReloadClipId" type="atString"/>
    <string name="WeaponAimReloadEmptyClipId" type="atString"/>
    <string name="WeaponIdleReloadClipId" type="atString"/>
    <string name="WeaponIdleReloadEmptyClipId" type="atString"/>
    <string name="WeaponLowLeftCoverReloadClipId" type="atString"/>
    <string name="WeaponLowRightCoverReloadClipId" type="atString"/>
  </structdef>
  <structdef type="CWeaponSwapData">
    <float name="AnimPlaybackRate"/>
    <string name="Name" type="atString"/>
    <string name="PedHolsterClipId" type="atString"/>
    <string name="PedHolsterCoverClipId" type="atString"/>
    <string name="PedHolsterCrouchClipId" type="atString"/>
    <string name="PedHolsterCrouchDiscardClipId" type="atString"/>
    <string name="PedHolsterDiscardClipId" type="atString"/>
    <string name="PedHolsterWeaponClipId" type="atString"/>
    <string name="PedUnHolsterClipId" type="atString"/>
    <string name="PedUnHolsterCrouchClipId" type="atString"/>
    <string name="PedUnHolsterLeftCoverClipId" type="atString"/>
    <string name="PedUnHolsterRightCoverClipId" type="atString"/>
    <string name="PedUnHolsterWeaponClipId" type="atString"/>
  </structdef>
  <structdef type="CWeaponComponentReloadLoopedData">
    <string name="Name" type="atString"/>
  </structdef>
  <structdef type="CWeaponComponentInfo">
    <bool name="ApplyWeaponTint"/>
    <string name="AttachBone" type="atString"/>
    <bool name="CreateObject"/>
    <s32 name="HudAccuracy"/>
    <s32 name="HudCapacity"/>
    <s32 name="HudDamage"/>
    <s32 name="HudRange"/>
    <s32 name="HudSpeed"/>
    <string name="LocDesc" type="atString"/>
    <string name="LocName" type="atString"/>
    <string name="Model" type="atString"/>
    <string name="Name" type="atString"/>
    <bool name="bShownOnWheel"/>
  </structdef>
  <structdef type="CWeaponComponentClipInfo">
    <string name="AmmoInfo" type="atString"/>
    <string name="AttachBone" type="atString"/>
    <s32 name="BulletsInBatch"/>
    <s32 name="ClipSize"/>
    <bool name="CreateObject"/>
    <s32 name="HudAccuracy"/>
    <s32 name="HudCapacity"/>
    <s32 name="HudDamage"/>
    <s32 name="HudRange"/>
    <s32 name="HudSpeed"/>
    <string name="LocDesc" type="atString"/>
    <string name="LocName" type="atString"/>
    <string name="Model" type="atString"/>
    <string name="Name" type="atString"/>
    <string name="ShellFx" type="atString"/>
    <string name="ShellFxFP" type="atString"/>
    <string name="TracerFx" type="atString"/>
    <s32 name="TracerFxForceLast"/>
    <s32 name="TracerFxSpacing"/>
    <bool name="bShownOnWheel"/>
  </structdef>
  <structdef type="CWeaponComponentFlashLightInfo">
    <string name="AttachBone" type="atString"/>
    <bool name="CreateObject"/>
    <string name="FlashLightBone" type="atString"/>
    <string name="FlashLightBoneBulbOff" type="atString"/>
    <string name="FlashLightBoneBulbOn" type="atString"/>
    <s32 name="HudAccuracy"/>
    <s32 name="HudCapacity"/>
    <s32 name="HudDamage"/>
    <s32 name="HudRange"/>
    <s32 name="HudSpeed"/>
    <string name="LocDesc" type="atString"/>
    <string name="LocName" type="atString"/>
    <float name="MainLightCoronaIntensity"/>
    <float name="MainLightCoronaSize"/>
    <float name="MainLightFalloffExponent"/>
    <float name="MainLightInnerAngle"/>
    <float name="MainLightIntensity"/>
    <float name="MainLightOuterAngle"/>
    <float name="MainLightRange"/>
    <float name="MainLightShadowFadeDistance"/>
    <float name="MainLightSpecularFadeDistance"/>
    <float name="MainLightVolumeExponent"/>
    <float name="MainLightVolumeIntensity"/>
    <float name="MainLightVolumeSize"/>
    <string name="Model" type="atString"/>
    <string name="Name" type="atString"/>
    <float name="SecondaryLightFadeDistance"/>
    <float name="SecondaryLightFalloffExponent"/>
    <float name="SecondaryLightInnerAngle"/>
    <float name="SecondaryLightIntensity"/>
    <float name="SecondaryLightOuterAngle"/>
    <float name="SecondaryLightRange"/>
    <float name="SecondaryLightVolumeExponent"/>
    <float name="SecondaryLightVolumeIntensity"/>
    <float name="SecondaryLightVolumeSize"/>
    <bool name="ToggleWhenAiming"/>
    <bool name="bShownOnWheel"/>
    <float name="fTargetDistalongAimCamera"/>
  </structdef>
  <structdef type="CWeaponComponentScopeInfo">
    <string name="AttachBone" type="atString"/>
    <string name="CameraHash" type="atString"/>
    <bool name="CreateObject"/>
    <float name="ExtraZoomFactorForAccurateMode"/>
    <s32 name="HudAccuracy"/>
    <s32 name="HudCapacity"/>
    <s32 name="HudDamage"/>
    <s32 name="HudRange"/>
    <s32 name="HudSpeed"/>
    <string name="LocDesc" type="atString"/>
    <string name="LocName" type="atString"/>
    <string name="Model" type="atString"/>
    <string name="Name" type="atString"/>
    <float name="RecoilShakeAmplitude"/>
    <s32 name="Reticule"/>
    <string name="ReticuleHash" type="atString"/>
    <string name="SpecialScopeType" type="atString"/>
    <bool name="bShownOnWheel"/>
  </structdef>
  <structdef type="CWeaponComponentSuppressorInfo">
    <bool name="ApplyWeaponTint"/>
    <string name="AttachBone" type="atString"/>
    <bool name="CreateObject"/>
    <string name="FlashFX" type="atString"/>
    <string name="FlashFx" type="atString"/>
    <s32 name="HudAccuracy"/>
    <s32 name="HudCapacity"/>
    <s32 name="HudDamage"/>
    <s32 name="HudRange"/>
    <s32 name="HudSpeed"/>
    <string name="LocDesc" type="atString"/>
    <string name="LocName" type="atString"/>
    <string name="Model" type="atString"/>
    <string name="MuzzleBone" type="atString"/>
    <string name="Name" type="atString"/>
    <float name="RecoilShakeAmplitudeModifier"/>
    <bool name="ShouldSilence"/>
    <bool name="bShownOnWheel"/>
  </structdef>
  <structdef type="CWeaponComponentVariantModelInfo">
    <string name="AttachBone" type="atString"/>
    <bool name="CreateObject"/>
    <s32 name="HudAccuracy"/>
    <s32 name="HudCapacity"/>
    <s32 name="HudDamage"/>
    <s32 name="HudRange"/>
    <s32 name="HudSpeed"/>
    <string name="LocDesc" type="atString"/>
    <string name="LocName" type="atString"/>
    <string name="Model" type="atString"/>
    <string name="Name" type="atString"/>
    <s32 name="TintIndexOverride"/>
    <bool name="bShownOnWheel"/>
  </structdef>
  <structdef type="CWeaponModelInfo__InitDataList">
    <array name="InitDatas" type="atArray"><struct type="CWeaponModelInfo__InitData"/></array>
  </structdef>
  <structdef type="CWeaponModelInfo__InitData">
    <string name="ExpressionDictionaryName" type="atString"/>
    <string name="ExpressionName" type="atString"/>
    <s32 name="lodDist"/>
    <string name="modelName" type="atString"/>
    <string name="ptfxAssetName" type="atString"/>
    <string name="txdName" type="atString"/>
  </structdef>
  <structdef type="CItemInfo">
  </structdef>
  <structdef type="CWeaponComponentData">
  </structdef></ParserSchema>"#;

/// [`WEAPON_TYPES`] as a library.
pub fn declared() -> &'static SchemaLibrary {
    static LIBRARY: OnceLock<SchemaLibrary> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        let mut library = SchemaLibrary::new();
        let items = parse_psc(WEAPON_TYPES.as_bytes(), "weapon")
            .expect("the weapon types are well-formed PSC");
        for item in items {
            match item {
                SchemaItem::Struct(def) => library.insert_struct(def),
                SchemaItem::Enum(def) => library.insert_enum(def),
                SchemaItem::Skipped => {}
            }
        }
        library
    })
}

/// `base` with the family's types.
///
/// ⚠ **The family's definition wins over a snapshot's**: a snapshot declares every member of a class, and this declares the ones the install's documents state with one shape, so a member it leaves out reads as the unread element it arrived as and is written back as it came.
pub fn library(base: &'static SchemaLibrary) -> Cow<'static, SchemaLibrary> {
    if base.is_empty() {
        return Cow::Borrowed(declared());
    }
    let mut merged = base.clone();
    let family = declared();
    merged
        .structs
        .extend(family.structs.iter().map(|(n, d)| (n.clone(), d.clone())));
    Cow::Owned(merged)
}

/// Whether `name` is a type of the family.
pub fn is_text_type(name: &str) -> bool {
    static NAMES: OnceLock<HashSet<u32>> = OnceLock::new();
    NAMES
        .get_or_init(|| declared().structs.keys().map(|n| name_hash(n)).collect())
        .contains(&name_hash(name))
}
