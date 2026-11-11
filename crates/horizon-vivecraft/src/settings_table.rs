// SPDX-License-Identifier: LGPL-3.0-or-later
// Generated from org.vivecraft.client_vr.settings.VRSettings (the @SettingField fields),
// Vivecraft commit 8b4d6b95a1891d9839f19b45c13e93415c02a31e. Copyright (c) Vivecraft contributors.
// Names are the config keys of vivecraft-client-config.json; ranges are the VrOptions slider ranges.
use super::{Def, Kind};

pub const AIM_DEVICE: &[&str] = &["CONTROLLER", "HMD"];
pub const AUTO_OPEN_KEYBOARD: &[&str] = &["OFF", "CHAT", "ON"];
pub const BOW_MODE: &[&str] = &["OFF", "VANILLA", "ON"];
pub const CHAT_NOTIFICATIONS: &[&str] = &["NONE", "HAPTIC", "SOUND", "BOTH"];
pub const CHAT_SERVER_MESSAGE: &[&str] = &["ALWAYS", "SERVER_ONLY", "NEVER"];
/// Horizon only: which VR protocol to speak to a server.
pub const VR_PROTOCOL: &[&str] = &["AUTO", "VISOR", "VIVECRAFT"];
/// Horizon only: Visor's `HeightMode`.
pub const VISOR_HEIGHT_MODE: &[&str] = &["MATCH_MODEL", "REAL_SIZE"];
/// Horizon only: Visor's `MovementMode`.
pub const VISOR_MOVEMENT: &[&str] = &["CONTROLLER", "TELEPORT"];
/// Horizon only: Visor's `VRBodyType` ids the player can pick.
pub const VISOR_BODY_TYPE: &[&str] = &["HANDS_ONLY", "FULL_BODY"];
/// Horizon only: how much of the headset frame's breakdown is shown (`vr_profile`).
pub const VR_PROFILE: &[&str] = &["OFF", "LOG", "OVERLAY"];
/// Horizon only: fixed foveation through XR_FB_foveation and a fragment density map (needs the runtime's four extensions and the GPU's VK_EXT_fragment_density_map); read at start, off
pub const VR_FOVEATION: &[&str] = &["OFF", "LOW", "MEDIUM", "HIGH"];
pub const DATA_SOURCE: &[&str] = &["REALTIME", "LOCAL", "SERVER"];
pub const FREE_MOVE: &[&str] = &["CONTROLLER", "HMD", "RUN_IN_PLACE", "ROOM", "WAIST", "AUTO"];
pub const VISOR_HEIGHT_ADJUSTMENT: &[&str] = &["CAMERA_OFFSET", "WORLD_SCALE"];
pub const VISOR_HEIGHT_UNITS: &[&str] = &["METRIC", "IMPERIAL"];
pub const HUD_LOCK: &[&str] = &["WRIST", "HAND", "HEAD"];
pub const INERTIA_FACTOR: &[&str] = &["NONE", "NORMAL", "LARGE", "MASSIVE"];
pub const KEYBOARD_THEME: &[&str] = &[
    "DEFAULT",
    "RED",
    "GREEN",
    "BLUE",
    "BLACK",
    "GRASS",
    "BEES",
    "AESTHETIC",
    "DOSE",
    "CUSTOM",
];
pub const MENU_WORLD: &[&str] = &["BOTH", "CUSTOM", "OFFICIAL", "NONE"];
pub const MODEL_ARMS_MODE: &[&str] = &["OFF", "SHOULDERS", "COMPLETE"];
pub const PLAYER_MODEL_TYPE: &[&str] = &["VANILLA", "SPLIT_ARMS", "SPLIT_ARMS_LEGS"];
pub const REALISTIC_JUMP: &[&str] = &["OFF", "ON", "AUTO"];
pub const RENDER_POINTER_ELEMENT: &[&str] = &["ALWAYS", "WITH_HUD", "NEVER"];
pub const RIGHT_CLICK_DELAY: &[&str] = &["VANILLA", "SLOW", "SLOWER", "SLOWEST"];
pub const SHADER_GUI_RENDER: &[&str] = &[
    "BEFORE_TRANSLUCENT_SOLID",
    "AFTER_TRANSLUCENT",
    "AFTER_SHADER",
];
pub const UPDATE_TYPE: &[&str] = &["RELEASE", "BETA", "ALPHA"];
pub const WEAPON_COLLISION: &[&str] = &["OFF", "ON", "AUTO"];

pub const QUEST_BUTTON: &[&str] = &[
    "NONE",
    "MAIN_TRIGGER",
    "OFF_TRIGGER",
    "MAIN_PRIMARY",
    "MAIN_SECONDARY",
    "OFF_PRIMARY",
    "OFF_SECONDARY",
    "MAIN_GRIP",
    "OFF_GRIP",
    "MAIN_STICK",
    "OFF_STICK",
];
pub const QUEST_STICK: &[&str] = &["MAIN", "OFF"];

pub const DEFS: &[Def] = &[
    Def {
        key: "questBindOffAttack",
        kind: Kind::Enum(QUEST_BUTTON),
        default: "OFF_TRIGGER",
        range: None,
    },
    Def {
        key: "questBindJump",
        kind: Kind::Enum(QUEST_BUTTON),
        default: "MAIN_SECONDARY",
        range: None,
    },
    Def {
        key: "questBindAttack",
        kind: Kind::Enum(QUEST_BUTTON),
        default: "MAIN_TRIGGER",
        range: None,
    },
    Def {
        key: "questBindUse",
        kind: Kind::Enum(QUEST_BUTTON),
        default: "MAIN_PRIMARY",
        range: None,
    },
    Def {
        key: "questBindOffUse",
        kind: Kind::Enum(QUEST_BUTTON),
        default: "OFF_PRIMARY",
        range: None,
    },
    Def {
        key: "questBindInventory",
        kind: Kind::Enum(QUEST_BUTTON),
        default: "NONE",
        range: None,
    },
    Def {
        key: "questBindDrop",
        kind: Kind::Enum(QUEST_BUTTON),
        default: "NONE",
        range: None,
    },
    Def {
        key: "questBindSwap",
        kind: Kind::Enum(QUEST_BUTTON),
        default: "NONE",
        range: None,
    },
    Def {
        key: "questBindChat",
        kind: Kind::Enum(QUEST_BUTTON),
        default: "NONE",
        range: None,
    },
    Def {
        key: "questBindKeyboard",
        kind: Kind::Enum(QUEST_BUTTON),
        default: "NONE",
        range: None,
    },
    Def {
        key: "questBindSneak",
        kind: Kind::Enum(QUEST_BUTTON),
        default: "MAIN_STICK",
        range: None,
    },
    Def {
        key: "questBindSprint",
        kind: Kind::Enum(QUEST_BUTTON),
        default: "NONE",
        range: None,
    },
    Def {
        key: "questBindMainHotbar",
        kind: Kind::Enum(QUEST_BUTTON),
        default: "MAIN_GRIP",
        range: None,
    },
    Def {
        key: "questBindOffHotbar",
        kind: Kind::Enum(QUEST_BUTTON),
        default: "OFF_GRIP",
        range: None,
    },
    Def {
        key: "questBindMenu",
        kind: Kind::Enum(QUEST_BUTTON),
        default: "OFF_SECONDARY",
        range: None,
    },
    Def {
        key: "questMoveStick",
        kind: Kind::Enum(QUEST_STICK),
        default: "OFF",
        range: None,
    },
    Def {
        key: "questTurnStick",
        kind: Kind::Enum(QUEST_STICK),
        default: "MAIN",
        range: None,
    },
    Def {
        key: "smoothRunTickCount",
        kind: Kind::Int,
        default: "20",
        range: None,
    },
    Def {
        key: "smoothTick",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "reverseHands",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "reverseShootingEye",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "aimDevice",
        kind: Kind::Enum(AIM_DEVICE),
        default: "CONTROLLER",
        range: None,
    },
    Def {
        key: "worldScale",
        kind: Kind::Float,
        default: "1.0",
        range: Some((0f32, 29f32, 1f32)),
    },
    Def {
        key: "worldRotation",
        kind: Kind::Float,
        default: "0.0",
        range: Some((0f32, 360f32, 30f32)),
    },
    Def {
        key: "vrWorldRotationIncrement",
        kind: Kind::Float,
        default: "45.0",
        range: Some((-1f32, 4f32, 1f32)),
    },
    Def {
        key: "worldRotationXSensitivity",
        kind: Kind::Float,
        default: "1.0",
        range: Some((0.1f32, 5f32, 0.01f32)),
    },
    Def {
        key: "xSensitivity",
        kind: Kind::Float,
        default: "1.0",
        range: Some((0.1f32, 5f32, 0.01f32)),
    },
    Def {
        key: "ySensitivity",
        kind: Kind::Float,
        default: "1.0",
        range: Some((0.1f32, 5f32, 0.01f32)),
    },
    Def {
        key: "keyholeX",
        kind: Kind::Float,
        default: "15.0",
        range: Some((0f32, 40f32, 5f32)),
    },
    Def {
        key: "headToHmdLength",
        kind: Kind::Float,
        default: "0.10",
        range: None,
    },
    Def {
        key: "autoCalibration",
        kind: Kind::Float,
        default: "-1.0",
        range: None,
    },
    Def {
        key: "manualCalibration",
        kind: Kind::Float,
        default: "-1.0",
        range: None,
    },
    Def {
        key: "fbtCalibrated",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "fbtExtendedCalibrated",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "unlabeledTrackersUsed",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "oscTrackerPort",
        kind: Kind::Int,
        default: "9000",
        range: Some((0f32, 65535f32, 1f32)),
    },
    Def {
        key: "alwaysSimulateKeyboard",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "bowMode",
        kind: Kind::Enum(BOW_MODE),
        default: "ON",
        range: None,
    },
    Def {
        key: "hrtfSelection",
        kind: Kind::Int,
        default: "0",
        range: None,
    },
    Def {
        key: "rightclickDelay",
        kind: Kind::Enum(RIGHT_CLICK_DELAY),
        default: "VANILLA",
        range: None,
    },
    Def {
        key: "ingameBindingsInGui",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "thirdPersonItems",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "thirdPersonItemsCustom",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "inertiaFactor",
        kind: Kind::Enum(INERTIA_FACTOR),
        default: "NORMAL",
        range: None,
    },
    Def {
        key: "walkUpBlocks",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "simulateFalling",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "weaponCollisionNew",
        kind: Kind::Enum(WEAPON_COLLISION),
        default: "AUTO",
        range: None,
    },
    Def {
        key: "feetCollision",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "blockCollision",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "entityCollision",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "swordBlockCollision",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "onlySwordCollision",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "reducedPlayerReach",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "allowBreakingClimbable",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "roomscaleSpearLunge",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "movementSpeedMultiplier",
        kind: Kind::Float,
        default: "1.0",
        range: Some((0.15f32, 1.3f32, 0.01f32)),
    },
    Def {
        key: "sprintMovementSpeedMultiplier",
        kind: Kind::Float,
        default: "0.0",
        range: Some((0.14f32, 1.3f32, 0.01f32)),
    },
    Def {
        key: "vrFreeMoveMode",
        kind: Kind::Enum(FREE_MOVE),
        default: "CONTROLLER",
        range: None,
    },
    Def {
        key: "vrFreeMoveFlyMode",
        kind: Kind::Enum(FREE_MOVE),
        default: "AUTO",
        range: None,
    },
    Def {
        key: "limitedTeleport",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "teleportLimitUp",
        kind: Kind::Int,
        default: "1",
        range: Some((0f32, 4f32, 1f32)),
    },
    Def {
        key: "teleportLimitDown",
        kind: Kind::Int,
        default: "4",
        range: Some((0f32, 16f32, 1f32)),
    },
    Def {
        key: "teleportLimitHoriz",
        kind: Kind::Int,
        default: "16",
        range: Some((0f32, 32f32, 1f32)),
    },
    Def {
        key: "seated",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "seatedhmd",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "jumpThreshold",
        kind: Kind::Float,
        default: "0.05",
        range: None,
    },
    Def {
        key: "sneakThreshold",
        kind: Kind::Float,
        default: "0.4",
        range: None,
    },
    Def {
        key: "crawlThreshold",
        kind: Kind::Float,
        default: "0.82",
        range: None,
    },
    Def {
        key: "realisticJumpEnabled",
        kind: Kind::Enum(REALISTIC_JUMP),
        default: "AUTO",
        range: None,
    },
    Def {
        key: "realisticSneakEnabled",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "realisticClimbEnabled",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "climbingAutoGrab",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "vanillaClimbing",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "realisticSwimEnabled",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "realisticRowEnabled",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "realisticDismountEnabled",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "realisticBlockInteractEnabled",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "realisticEntityInteractEnabled",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "doorHitting",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "backpackSwitching",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "backpackMainHandKeybind",
        kind: Kind::Str,
        default: "key.vivecraft.quickSwap",
        range: None,
    },
    Def {
        key: "backpackOffhandKeybind",
        kind: Kind::Str,
        default: "key.swapOffhand",
        range: None,
    },
    Def {
        key: "physicalGuiEnabled",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "walkMultiplier",
        kind: Kind::Float,
        default: "1.0",
        range: Some((1f32, 10f32, 0.1f32)),
    },
    Def {
        key: "allowCrawling",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "bcbOn",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "feetBodyPosition",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "vehicleRotation",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "analogMovement",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "digitalMovementDeadzone",
        kind: Kind::Float,
        default: "0.5",
        range: Some((0.0f32, 1f32, 0.05f32)),
    },
    Def {
        key: "autoSprint",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "autoSprintThreshold",
        kind: Kind::Float,
        default: "0.9",
        range: Some((0.5f32, 1f32, 0.01f32)),
    },
    Def {
        key: "originOffset",
        kind: Kind::Vec3,
        default: "0.0,0.0,0.0",
        range: None,
    },
    Def {
        key: "allowStandingOriginOffset",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "seatedFreeMove",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "forceStandingFreeMove",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "useFsaa",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "low_health_indicator",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "hitIndicator",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "waterEffect",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "portalEffect",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "freezeEffect",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "pumpkinEffect",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "sleepEffect",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "fovReduction",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "fovRedutioncOffset",
        kind: Kind::Float,
        default: "0.1",
        range: Some((0.0f32, 0.3f32, 0.01f32)),
    },
    Def {
        key: "fovReductionMin",
        kind: Kind::Float,
        default: "0.25",
        range: Some((0.1f32, 0.7f32, 0.05f32)),
    },
    Def {
        key: "stencilOn",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "stencilBufferDisable",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "insideBlockSolidColor",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "renderScaleFactor",
        kind: Kind::Float,
        default: "1.0",
        range: Some((0.25f32, 2f32, 0.05f32)),
    },
    Def {
        key: "shouldRenderSelf",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "mainPlayerDataSource",
        kind: Kind::Enum(DATA_SOURCE),
        default: "REALTIME",
        range: None,
    },
    Def {
        key: "playerModelType",
        kind: Kind::Enum(PLAYER_MODEL_TYPE),
        default: "VANILLA",
        range: None,
    },
    Def {
        key: "modelArmsMode",
        kind: Kind::Enum(MODEL_ARMS_MODE),
        default: "COMPLETE",
        range: None,
    },
    Def {
        key: "showPlayerHands",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "playerLimbsConnected",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "playerLimbsLimit",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "playerWalkAnim",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "playerArmAnim",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "playerModelArmsScale",
        kind: Kind::Float,
        default: "0.5",
        range: Some((0.1f32, 1f32, 0.05f32)),
    },
    Def {
        key: "playerModelBodyScale",
        kind: Kind::Float,
        default: "1.0",
        range: Some((0.1f32, 1f32, 0.05f32)),
    },
    Def {
        key: "playerModelLegScale",
        kind: Kind::Float,
        default: "1.0",
        range: Some((0.1f32, 1f32, 0.05f32)),
    },
    Def {
        key: "applyPlayerWorldscale",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "menuWorldSelection",
        kind: Kind::Enum(MENU_WORLD),
        default: "BOTH",
        range: None,
    },
    Def {
        key: "menuWorldFallbackPanorama",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "renderHeadHitbox",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "renderDeviceAxes",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "renderVrPlayerAxes",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "renderTrackerPositions",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "renderGameplayTrackers",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "gameplayTrackerToRender",
        kind: Kind::Str,
        default: "",
        range: None,
    },
    Def {
        key: "renderAllPasses",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "vrTouchHotbar",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "headHudScale",
        kind: Kind::Float,
        default: "1.0",
        range: Some((0.35f32, 2.5f32, 0.01f32)),
    },
    Def {
        key: "hudDistance",
        kind: Kind::Float,
        default: "1.25",
        range: Some((0.25f32, 5.0f32, 0.01f32)),
    },
    Def {
        key: "hudPitchOffset",
        kind: Kind::Float,
        default: "-2.0",
        range: None,
    },
    Def {
        key: "hudYawOffset",
        kind: Kind::Float,
        default: "0.0",
        range: None,
    },
    Def {
        key: "menuAlwaysFollowFace",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "vrHudLockMode",
        kind: Kind::Enum(HUD_LOCK),
        default: "WRIST",
        range: None,
    },
    Def {
        key: "vrHudWristOffset",
        kind: Kind::Float,
        default: "1.0",
        range: Some((0.0f32, 4.0f32, 0.25f32)),
    },
    Def {
        key: "forceGuiToHUD",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "hudOcclusion",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "crosshairScale",
        kind: Kind::Float,
        default: "1.0",
        range: Some((0.25f32, 1.0f32, 0.01f32)),
    },
    Def {
        key: "crosshairScalesWithDistance",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "renderInGameCrosshairMode",
        kind: Kind::Enum(RENDER_POINTER_ELEMENT),
        default: "ALWAYS",
        range: None,
    },
    Def {
        key: "renderBlockOutlineMode",
        kind: Kind::Enum(RENDER_POINTER_ELEMENT),
        default: "ALWAYS",
        range: None,
    },
    Def {
        key: "hudOpacity",
        kind: Kind::Float,
        default: "1.0",
        range: Some((0.15f32, 1.0f32, 0.05f32)),
    },
    Def {
        key: "menuBackground",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "menuCrosshairScale",
        kind: Kind::Float,
        default: "1.0",
        range: Some((0.25f32, 2.5f32, 0.05f32)),
    },
    Def {
        key: "useCrosshairOcclusion",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "seatedHudAltMode",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "autoOpenKeyboard",
        kind: Kind::Enum(AUTO_OPEN_KEYBOARD),
        default: "OFF",
        range: None,
    },
    Def {
        key: "autoCloseKeyboard",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "forceHardwareDetection",
        kind: Kind::Int,
        default: "0",
        range: None,
    },
    Def {
        key: "radialModeHold",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "radialRepeat",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "vrRadialButtons",
        kind: Kind::Int,
        default: "8",
        range: Some((4f32, 14f32, 2f32)),
    },
    Def {
        key: "physicalKeyboard",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "physicalKeyboardScale",
        kind: Kind::Float,
        default: "1.0",
        range: Some((0.75f32, 1.5f32, 0.01f32)),
    },
    Def {
        key: "physicalKeyboardTheme",
        kind: Kind::Enum(KEYBOARD_THEME),
        default: "DEFAULT",
        range: None,
    },
    Def {
        key: "keyboardPressBinds",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "keyboardShowLayoutSelect",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "allowAdvancedBindings",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "chatNotifications",
        kind: Kind::Enum(CHAT_NOTIFICATIONS),
        default: "NONE",
        range: None,
    },
    Def {
        key: "chatNotificationSound",
        kind: Kind::Str,
        default: "block.note_block.bell",
        range: None,
    },
    Def {
        key: "guiAppearOverBlock",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "shaderGUIRender",
        kind: Kind::Enum(SHADER_GUI_RENDER),
        default: "AFTER_SHADER",
        range: None,
    },
    Def {
        key: "shaderFullSizeShadowLimbs",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "disableShaderOptimization",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "shaderPatching",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "doubleGUIResolution",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "guiScale",
        kind: Kind::Int,
        default: "0",
        range: Some((0f32, 6f32, 1f32)),
    },
    Def {
        key: "hudMaxScale",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "guiMipmaps",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "guiAnisotropicFiltering",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "alwaysShowUpdates",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "lastUpdate",
        kind: Kind::Str,
        default: "",
        range: None,
    },
    Def {
        key: "updateType",
        kind: Kind::Enum(UPDATE_TYPE),
        default: "RELEASE",
        range: None,
    },
    Def {
        key: "showServerPluginMessage",
        kind: Kind::Enum(CHAT_SERVER_MESSAGE),
        default: "SERVER_ONLY",
        range: None,
    },
    Def {
        key: "showServerPluginMissingMessageAlways",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "showServerVrChangesMessage",
        kind: Kind::Enum(CHAT_SERVER_MESSAGE),
        default: "SERVER_ONLY",
        range: None,
    },
    Def {
        key: "showChatMessageStencil",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "vrEnabled",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "closeWithRuntime",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "rememberVr",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "vrHotswitchingEnabled",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "vrToggleButtonEnabled",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "vrSettingsButtonEnabled",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "vrSettingsButtonPositionLeft",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "commandsButtonIcon",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "modifyPauseMenu",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "fullReloadOnInit",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "useFuzzySearch",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "disableGarbageCollectorMessage",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "selfButtSparklesInFirstPerson",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "blockIntelWindows",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "debugCameraTracker",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    // The array settings of VRSettings, one entry per line: the radial menu's items (14 each,
    // empty = unused), the quick commands (12), the keyboard layouts in use and the current one.
    Def {
        key: "vrRadialItems",
        kind: Kind::Str,
        default: "key.drop\nkey.chat\nvivecraft.key.rotateRight\nkey.pickItem\nvivecraft.key.toggleHandheldCam\nvivecraft.key.togglePlayerList\nvivecraft.key.rotateLeft\nvivecraft.key.quickTorch\n\n\n\n\n\n",
        range: None,
    },
    Def {
        key: "vrRadialItemsAlt",
        kind: Kind::Str,
        default: "\n\n\n\n\n\n\n\n\n\n\n\n\n",
        range: None,
    },
    Def {
        key: "vrQuickCommands",
        kind: Kind::Str,
        default: "/gamemode survival\n/gamemode creative\n/help\n/home\n/sethome\n/spawn\nhi!\nbye!\nfollow me!\ntake this!\nthank you!\npraise the sun!",
        range: None,
    },
    Def {
        key: "keyboardLayoutOrder",
        kind: Kind::Str,
        default: "custom\nen_us",
        range: None,
    },
    Def {
        key: "currentKeyboardLayout",
        kind: Kind::Int,
        default: "0",
        range: None,
    },
    // Optional compositor hint; does not reduce the saved render scale.
    Def {
        key: "horizonSuperResolution",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    // Experimental stereo world pass, with automatic compatibility fallback.
    Def {
        key: "horizonMultiview",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    // Horizon only: the display refresh rate asked for (Hz), the nearest the headset offers.
    Def {
        key: "horizonRefreshRate",
        kind: Kind::Float,
        default: "90.0",
        range: Some((72f32, 120f32, 6f32)),
    },
    // Horizon only: walk in the room by Vivecraft's `doPlayerMoveInRoom` rules (see
    // `Game::headset_room_walk`) instead of Horizon's own slide-along-walls step.
    Def {
        key: "horizonVivecraftRoomWalk",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    // Horizon only: the VR protocol spoken to servers. AUTO says Visor's HANDSHAKE first and
    // falls back to Vivecraft's VERSION when no Visor answer comes.
    Def {
        key: "horizonVrProtocol",
        kind: Kind::Enum(VR_PROTOCOL),
        default: "AUTO",
        range: None,
    },
    // Horizon only, Visor: `VRClientSettings.heightMode` (the server's supported mode wins).
    Def {
        key: "visorHeightMode",
        kind: Kind::Enum(VISOR_HEIGHT_MODE),
        default: "MATCH_MODEL",
        range: None,
    },
    // Horizon only, Visor: `defaultVrBody`: the body other players see you with.
    Def {
        key: "visorBodyType",
        kind: Kind::Enum(VISOR_BODY_TYPE),
        default: "HANDS_ONLY",
        range: None,
    },
    // Horizon only, Visor: `handTrackingEnabled` (`XR_EXT_hand_tracking`; needs the headset's
    // hand tracking and the server's `handTrackersSupported`).
    Def {
        key: "visorHandTracking",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    // Horizon only, Visor: `fbtEnabled` (`XR_FB_body_tracking`; needs the server's
    // `bodyTrackersSupported`).
    Def {
        key: "visorBodyTracking",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    // Horizon only: which look-and-feel profile the file was last brought to (0 none, 1 Visor's
    // defaults, see `settings::VISOR_PROFILE`); the game moves an older file over once.
    Def {
        key: "visorProfile",
        kind: Kind::Int,
        default: "0",
        range: None,
    },
    // Horizon only, Visor: the width in metres the player resized a screen to by its handles
    // (`OverlayOptionsResizing.resizingScale`); -1 until then. Not on any page.
    Def {
        key: "visorScreenScale",
        kind: Kind::Float,
        default: "-1.0",
        range: None,
    },
    // Horizon only, Visor: `VRClientSettings.movementMode`.
    Def {
        key: "visorMovementMode",
        kind: Kind::Enum(VISOR_MOVEMENT),
        default: "CONTROLLER",
        range: None,
    },
    // Horizon only, Visor's height settings (`VRClientSettings.heightAdjustment`, `heightUnits`,
    // `heightAuto`): how the measured height is applied, how it is shown, and whether the game
    // finds it out by itself (`PlayerHeightTracker`).
    Def {
        key: "visorHeightAdjustment",
        kind: Kind::Enum(VISOR_HEIGHT_ADJUSTMENT),
        default: "CAMERA_OFFSET",
        range: None,
    },
    Def {
        key: "visorHeightUnits",
        kind: Kind::Enum(VISOR_HEIGHT_UNITS),
        default: "METRIC",
        range: None,
    },
    Def {
        key: "visorHeightAuto",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    // Horizon only, Visor's room thresholds as a share of the standing height
    // (`room_sneak.threshold` 0.85, `room_crawl.threshold` 0.7, `room_jump.threshold` 1.05) and
    // `swing.in_creative`, `room_consume`.
    Def {
        key: "visorSneakThreshold",
        kind: Kind::Float,
        default: "0.85",
        range: Some((0.5f32, 1.0f32, 0.01f32)),
    },
    Def {
        key: "visorCrawlThreshold",
        kind: Kind::Float,
        default: "0.7",
        range: Some((0.3f32, 0.9f32, 0.01f32)),
    },
    Def {
        key: "visorJumpThreshold",
        kind: Kind::Float,
        default: "1.05",
        range: Some((1.0f32, 1.3f32, 0.01f32)),
    },
    Def {
        key: "visorSwingInCreative",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "visorRoomConsume",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    // Horizon only, Visor: the hotbar wheel (`center_radius`, `hysteresis_margin`, `slot_numbers`) and
    // `hud_disabled` read the other way (the vanilla hotbar is left out of the HUD by default).
    Def {
        key: "visorHotbarCenterRadius",
        kind: Kind::Float,
        default: "50.0",
        range: Some((10f32, 120f32, 1f32)),
    },
    Def {
        key: "visorHotbarHysteresis",
        kind: Kind::Float,
        default: "7.0",
        range: Some((0f32, 20f32, 1f32)),
    },
    Def {
        key: "visorHotbarSlotNumbers",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    Def {
        key: "visorHudHotbar",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    // Horizon only, Visor: `swing.speed_threshold` (m/s of the tip a swing needs).
    Def {
        key: "visorSwingSpeedThreshold",
        kind: Kind::Float,
        default: "3.0",
        range: Some((0.5f32, 8.0f32, 0.1f32)),
    },
    Def {
        key: "visorKeyboardScale",
        kind: Kind::Float,
        default: "0.5",
        range: Some((0.25, 1.5, 0.05)),
    },
    Def {
        key: "visorChatVisible",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "visorChatX",
        kind: Kind::Float,
        default: "0.392",
        range: Some((-2.0, 2.0, 0.05)),
    },
    Def {
        key: "visorChatY",
        kind: Kind::Float,
        default: "0.214",
        range: Some((-2.0, 2.0, 0.05)),
    },
    Def {
        key: "visorChatZ",
        kind: Kind::Float,
        default: "-1.706",
        range: Some((-4.0, -0.25, 0.05)),
    },
    Def {
        key: "visorChatScale",
        kind: Kind::Float,
        default: "1.2",
        range: Some((0.25, 3.0, 0.05)),
    },
    // Horizon only, Visor: its keyboard (`VROverlayKeyboard`) for typing in a headset; off brings
    // back Horizon's own on-screen keyboard. `keyboard.auto_layout` adds the layout of the
    // game's language, `keyboard.layouts` lists the layouts the language button walks
    // (`KeyboardLayout` names, comma separated).
    Def {
        key: "visorKeyboard",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "visorKeyboardAutoLayout",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    Def {
        key: "visorKeyboardLayouts",
        kind: Kind::Str,
        default: "ENGLISH",
        range: None,
    },
    // Horizon only, Visor: the view darkens near a solid block (`VRShaderInBlockVignette`),
    // through the comfort vignette's image.
    Def {
        key: "visorInBlockVignette",
        kind: Kind::Bool,
        default: "true",
        range: None,
    },
    // Horizon only: shrink the eyes' images when frames come late and grow them back (off).
    Def {
        key: "horizonDynamicResolution",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    // Horizon only: the headset frame split into simulation, preparation, per-eye submit, the
    // runtime's waits and GPU timestamps, to the log (LOG) and to F3 (OVERLAY). Off by default.
    Def {
        key: "horizonVrProfile",
        kind: Kind::Enum(VR_PROFILE),
        default: "OFF",
        range: None,
    },
    // Horizon only: the render and server threads stay off the chip's slow cores (off)
    Def {
        key: "horizonPinBigCores",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    // Horizon only: terrain as one draw call per arena page with back faces left out, instead of one per section: for GPUs with no multi-draw (Quest's Adreno), off
    Def {
        key: "horizonFlatDraws",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    // Horizon only: milliseconds a frame may spend uploading chunk meshes in a headset's world
    // view (three times while the terrain arrives); 0 keeps the desktop's 2 ms and 6 ms.
    Def {
        key: "horizonVrUploadMs",
        kind: Kind::Float,
        default: "0.0",
        range: Some((0.0f32, 4.0f32, 0.5f32)),
    },
    // Horizon only: draw each eye's hidden area (XR_KHR_visibility_mask) at the nearest depth so nothing behind it is shaded; GPU only, off
    Def {
        key: "horizonVisibilityMask",
        kind: Kind::Bool,
        default: "false",
        range: None,
    },
    // Horizon only: fixed foveation through XR_FB_foveation and a fragment density map (needs the runtime's four extensions and the GPU's VK_EXT_fragment_density_map); read at start, off
    Def {
        key: "horizonFoveation",
        kind: Kind::Enum(VR_FOVEATION),
        default: "OFF",
        range: None,
    },
];
