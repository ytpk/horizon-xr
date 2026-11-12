// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports the page structure of Vivecraft's VR options screens
//! (`org.vivecraft.client.gui.settings.*`): which page lists which option, in Vivecraft's order.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).
//!
//! Left out because they edit data Horizon has no editor for yet: the keyboard layout and theme
//! editors, the controller binding list, the mixed reality, mirror and external camera options,
//! the server settings. The radial menu and quick command editors are in `radial`.

use crate::settings::VrSettings;

/// A page of the VR options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageId {
    /// `GuiMainVRSettings`.
    Main,
    /// `GuiHUDSettings`.
    Hud,
    /// `GuiOtherHUDSettings`.
    OtherHud,
    /// `GuiRenderOpticsSettings`.
    Rendering,
    /// `GuiPostEffectsSettings`.
    PostEffects,
    /// `GuiStandingSettings`.
    Standing,
    /// `GuiRoomscaleSettings`.
    Roomscale,
    /// `GuiWeaponCollisionSettings`.
    WeaponCollision,
    /// `GuiBackpackSwitchingSettings`.
    Backpack,
    /// `GuiClimbingSettings`.
    Climbing,
    /// `GuiFreeMoveSettings`.
    FreeMove,
    /// `GuiTeleportSettings`.
    Teleport,
    /// `GuiSeatedOptions`.
    Seated,
    /// `GuiPlayerModelSettings`.
    PlayerModel,
    /// `GuiKeyboardSettings`.
    Keyboard,
    /// `GuiRadialSettings`.
    Radial,
    /// `GuiMenuWorldSettings`.
    MenuWorld,
    /// Horizon only: the Visor protocol and the performance switches.
    Visor,
    /// Horizon only: Quest rendering experiments, read when the game starts.
    Experiments,
    // The pages of Visor's VR settings (`VRSettingsCategory` and the sub-screens its categories
    // open), in its order: Presets, Rendering, VR Body, Controls, Movement, Immersion, GUI, World.
    Presets,
    VRendering,
    Effects,
    EyeEffects,
    VBody,
    BodySelect,
    Height,
    Controls,
    Bindings,
    Movement,
    Immersion,
    ImmersionAdvanced,
    Gui,
    Hotbar,
    KeyboardLayouts,
    Overlays,
    World,
}

impl PageId {
    /// The page's title (`vrTitle`).
    pub fn title(self) -> &'static str {
        match self {
            PageId::Main => "visor.options.main",
            PageId::Hud => "vivecraft.options.screen.gui",
            PageId::OtherHud => "vivecraft.options.screen.guiother",
            PageId::Rendering => "vivecraft.options.screen.stereorendering",
            PageId::PostEffects => "vivecraft.options.screen.posteffects",
            PageId::Standing => "vivecraft.options.screen.standing",
            PageId::Roomscale => "vivecraft.options.screen.roomscale",
            PageId::WeaponCollision => "vivecraft.options.screen.weaponcollision",
            PageId::Backpack => "vivecraft.options.screen.backpackswitching",
            PageId::Climbing => "vivecraft.options.screen.climbing",
            PageId::FreeMove => "vivecraft.options.screen.freemove",
            PageId::Teleport => "vivecraft.options.screen.teleport",
            PageId::Seated => "vivecraft.options.screen.seated",
            PageId::PlayerModel => "vivecraft.options.screen.playermodel",
            PageId::Keyboard => "vivecraft.options.screen.keyboard",
            PageId::Radial => "vivecraft.options.screen.radialsettings",
            PageId::MenuWorld => "vivecraft.options.screen.menuworld",
            PageId::Visor => "horizon.visor.page",
            PageId::Experiments => "horizon.vr.experiments",
            PageId::Presets => "visor.options.presets.button",
            PageId::VRendering => "visor.options.rendering.button",
            PageId::Effects => "visor.options.rendering.effects.button",
            PageId::EyeEffects => "visor.options.rendering.effects.eye_effects.button",
            PageId::VBody => "visor.options.vr_body.button",
            PageId::BodySelect => "visor.options.vr_body.select",
            PageId::Height => "visor.options.vr_body.height.title",
            PageId::Controls => "visor.options.controls.button",
            PageId::Bindings => "horizon.vr.bindings",
            PageId::Movement => "visor.options.movement.button",
            PageId::Immersion => "visor.options.immersion.button",
            PageId::ImmersionAdvanced => "visor.options.immersion.advanced.button",
            PageId::Gui => "visor.options.gui.button",
            PageId::Hotbar => "visor.options.gui.hotbar.button",
            PageId::KeyboardLayouts => "horizon.visor.keyboard.layouts",
            PageId::Overlays => "horizon.visor.overlays",
            PageId::World => "visor.options.world.button",
        }
    }
    /// The label of the button that opens the page.
    pub fn button(self) -> &'static str {
        match self {
            PageId::Main => "visor.options.main.button",
            PageId::Hud => "vivecraft.options.screen.gui.button",
            PageId::OtherHud => "vivecraft.options.screen.guiother.button",
            PageId::Rendering => "vivecraft.options.screen.stereorendering.button",
            PageId::PostEffects => "vivecraft.options.screen.posteffects.button",
            PageId::Standing => "vivecraft.options.screen.standing.button",
            PageId::Roomscale => "vivecraft.options.screen.roomscale.button",
            PageId::WeaponCollision => "vivecraft.options.screen.weaponcollision.button",
            PageId::Backpack => "vivecraft.options.screen.backpackswitching.button",
            PageId::Climbing => "vivecraft.options.screen.climbing.button",
            PageId::FreeMove => "vivecraft.options.screen.freemove.button",
            PageId::Teleport => "vivecraft.options.screen.teleport.button",
            PageId::Seated => "vivecraft.options.screen.seated.button",
            PageId::PlayerModel => "vivecraft.options.screen.playermodel.button",
            PageId::Keyboard => "vivecraft.options.screen.keyboard.button",
            PageId::Radial => "vivecraft.options.screen.radialmenu.button",
            PageId::MenuWorld => "vivecraft.options.screen.menuworld.button",
            PageId::Visor => "horizon.visor.page.button",
            PageId::Experiments => "horizon.vr.experiments.button",
            PageId::Presets => "visor.options.presets.button",
            PageId::VRendering => "visor.options.rendering.button",
            PageId::Effects => "visor.options.rendering.effects.button",
            PageId::EyeEffects => "visor.options.rendering.effects.eye_effects.button",
            PageId::VBody => "visor.options.vr_body.button",
            PageId::BodySelect => "visor.options.vr_body.select.button",
            PageId::Height => "visor.options.vr_body.height.button",
            PageId::Controls => "visor.options.controls.button",
            PageId::Bindings => "horizon.vr.bindings",
            PageId::Movement => "visor.options.movement.button",
            PageId::Immersion => "visor.options.immersion.button",
            PageId::ImmersionAdvanced => "visor.options.immersion.advanced.button",
            PageId::Gui => "visor.options.gui.button",
            PageId::Hotbar => "visor.options.gui.hotbar.button",
            PageId::KeyboardLayouts => "horizon.visor.keyboard.layouts",
            PageId::Overlays => "horizon.visor.overlays",
            PageId::World => "visor.options.world.button",
        }
    }
}

/// One cell of a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    /// An option, by config key.
    Opt(&'static str),
    /// An option selected by name when several controls edit one serialized setting.
    NamedOpt(&'static str),
    /// A button that opens another page.
    Page(PageId),
    /// `RESET_ORIGIN`: put the room origin back under the player.
    ResetOrigin,
    /// `heightCalibration`: set the player's height from how they stand now.
    CalibrateHeight,
    /// `GuiRadialConfiguration`'s button (`radialmenu.button`).
    RadialConfig,
    /// `GuiQuickCommandEditor`'s button (`quickcommands.button`).
    QuickCommands,
    /// A button that lays a settings preset over the VR settings (Horizon's Visor page).
    Preset(&'static str),
    /// An empty cell.
    Blank,
}

use Entry::{Blank, Opt};

/// The page's cells, two to a row, left then right (Vivecraft's `POS_LEFT`/`POS_RIGHT`).
/// `server_limited_teleport` is `ClientNetworking.isLimitedSurvivalTeleport`.
pub fn entries(page: PageId, s: &VrSettings, server_limited_teleport: bool) -> Vec<Entry> {
    let seated = s.bool("seated");
    match page {
        // Visor's settings screen: the categories in the order of its tab list.
        PageId::Main => vec![
            Entry::Page(PageId::Presets),
            Entry::Page(PageId::VRendering),
            Entry::Page(PageId::VBody),
            Entry::Page(PageId::Controls),
            Entry::Page(PageId::Movement),
            Entry::Page(PageId::Immersion),
            Entry::Page(PageId::Gui),
            Entry::Page(PageId::World),
        ],
        PageId::Presets => vec![
            Entry::Preset("defaults"),
            Entry::Preset("defaults_hand_hud"),
        ],
        PageId::VRendering => vec![
            Opt("renderScaleFactor"),
            Opt("horizonMultiview"),
            Opt("horizonFoveatedMultiview"),
            Opt("horizonOpaqueWater"),
            Entry::Page(PageId::Experiments),
            Entry::Page(PageId::Effects),
            Entry::Page(PageId::Visor),
        ],
        PageId::Effects => vec![Entry::Page(PageId::EyeEffects)],
        PageId::EyeEffects => vec![
            Opt("low_health_indicator"),
            Opt("hitIndicator"),
            Opt("freezeEffect"),
            Opt("pumpkinEffect"),
        ],
        PageId::VBody => vec![
            Entry::Page(PageId::BodySelect),
            Entry::Page(PageId::Height),
            Opt("visorBodyTracking"),
            Opt("visorHandTracking"),
        ],
        PageId::BodySelect => vec![Opt("visorBodyType")],
        PageId::Height => vec![
            Opt("visorHeightMode"),
            Opt("visorHeightUnits"),
            Opt("visorHeightAuto"),
            Entry::CalibrateHeight,
            Entry::ResetOrigin,
        ],
        PageId::Controls => vec![
            Opt("reverseHands"),
            Entry::ResetOrigin,
            Entry::Page(PageId::Bindings),
        ],
        PageId::Bindings => vec![
            Entry::Preset("quest_controls"),
            Opt("questBindJump"),
            Opt("questBindAttack"),
            Opt("questBindOffAttack"),
            Opt("questBindUse"),
            Opt("questBindOffUse"),
            Opt("questBindInventory"),
            Opt("questBindDrop"),
            Opt("questBindSwap"),
            Opt("questBindChat"),
            Opt("questBindKeyboard"),
            Opt("questBindSneak"),
            Opt("questBindSprint"),
            Opt("questBindMainHotbar"),
            Opt("questBindOffHotbar"),
            Opt("questBindMenu"),
            Opt("questMoveStick"),
            Opt("questTurnStick"),
        ],
        PageId::Movement => vec![
            Opt("visorMovementMode"),
            Opt("vrFreeMoveMode"),
            Opt("vrFreeMoveFlyMode"),
            Opt("vrWorldRotationIncrement"),
            Opt("worldRotationXSensitivity"),
            Opt("walkUpBlocks"),
            Opt("autoSprint"),
            Opt("autoSprintThreshold"),
        ],
        PageId::Immersion => vec![
            Opt("realisticSneakEnabled"),
            Opt("allowCrawling"),
            Opt("realisticClimbEnabled"),
            Opt("realisticJumpEnabled"),
            Opt("realisticSwimEnabled"),
            Opt("realisticDismountEnabled"),
            Opt("realisticRowEnabled"),
            Opt("visorRoomConsume"),
            Entry::Page(PageId::ImmersionAdvanced),
        ],
        PageId::ImmersionAdvanced => vec![
            Opt("visorSneakThreshold"),
            Opt("visorCrawlThreshold"),
            Opt("visorJumpThreshold"),
            Opt("visorSwingSpeedThreshold"),
            Opt("visorSwingInCreative"),
        ],
        PageId::Gui => vec![
            Entry::Page(PageId::Hotbar),
            Entry::Page(PageId::KeyboardLayouts),
            Entry::Page(PageId::Overlays),
            Opt("autoOpenKeyboard"),
            Opt("hudDistance"),
            Opt("headHudScale"),
            Opt("hudOpacity"),
        ],
        PageId::Hotbar => vec![
            Opt("visorHotbarCenterRadius"),
            Opt("visorHotbarHysteresis"),
            Opt("visorHotbarSlotNumbers"),
            Opt("visorHudHotbar"),
        ],
        PageId::KeyboardLayouts => vec![
            Opt("visorKeyboardAutoLayout"),
            Opt("visorKeyboardScale"),
            Entry::NamedOpt("VISOR_KEYBOARD_LAYOUT_ENGLISH"),
            Entry::NamedOpt("VISOR_KEYBOARD_LAYOUT_RUSSIAN"),
            Entry::NamedOpt("VISOR_KEYBOARD_LAYOUT_UKRAINIAN"),
            Entry::NamedOpt("VISOR_KEYBOARD_LAYOUT_GERMAN"),
            Entry::NamedOpt("VISOR_KEYBOARD_LAYOUT_FRENCH"),
            Entry::NamedOpt("VISOR_KEYBOARD_LAYOUT_SPANISH"),
            Entry::NamedOpt("VISOR_KEYBOARD_LAYOUT_ITALIAN"),
            Entry::NamedOpt("VISOR_KEYBOARD_LAYOUT_PORTUGUESE"),
            Entry::NamedOpt("VISOR_KEYBOARD_LAYOUT_HUNGARIAN"),
        ],
        PageId::Overlays => vec![
            Opt("vrHudLockMode"),
            Opt("hudDistance"),
            Opt("headHudScale"),
            Opt("hudOpacity"),
            Opt("visorChatVisible"),
            Opt("visorChatX"),
            Opt("visorChatY"),
            Opt("visorChatZ"),
            Opt("visorChatScale"),
        ],
        // The server rules of a world (`VRServerSettings`) have no page; a note says where they live.
        PageId::World => Vec::new(),
        PageId::Visor => vec![
            Opt("horizonVrProtocol"),
            Opt("visorKeyboard"),
            Opt("visorKeyboardAutoLayout"),
            Opt("visorInBlockVignette"),
            Opt("horizonRefreshRate"),
            Opt("horizonDynamicResolution"),
            Opt("horizonOpaqueWater"),
            Opt("horizonVrProfile"),
            Opt("horizonFoveation"),
            Opt("horizonMultiview"),
            Opt("horizonFoveatedMultiview"),
            Opt("horizonVisibilityMask"),
            Opt("horizonVrUploadMs"),
            Opt("horizonFlatDraws"),
            Opt("horizonPinBigCores"),
            Entry::Page(PageId::Experiments),
        ],
        PageId::Experiments => vec![
            Opt("horizonLeanOpaque"),
            Opt("horizonLeanCutout"),
            Opt("horizonMergeFaces"),
            Opt("horizonSplitStreams"),
            Opt("horizonShrinkPlants"),
            Opt("horizonPlantDistance"),
        ],
        PageId::Hud => vec![
            Opt("vrHudLockMode"),
            Opt("headHudScale"),
            Opt("hudDistance"),
            Opt("hudOcclusion"),
            Opt("hudOpacity"),
            Opt("menuBackground"),
            Opt("vrTouchHotbar"),
            Opt("menuAlwaysFollowFace"),
            Opt("guiAppearOverBlock"),
            Opt("vrHudWristOffset"),
            Opt("forceGuiToHUD"),
            Entry::Page(PageId::Keyboard),
            Entry::Page(PageId::MenuWorld),
        ],
        PageId::OtherHud => {
            let mut v = vec![
                Opt("crosshairScale"),
                Opt("renderInGameCrosshairMode"),
                Opt("renderBlockOutlineMode"),
                Opt("menuCrosshairScale"),
                Opt("useCrosshairOcclusion"),
                Opt("crosshairScalesWithDistance"),
                Opt("chatNotifications"),
            ];
            let notif = s.enum_name("chatNotifications");
            v.push(if notif == "SOUND" || notif == "BOTH" {
                Opt("chatNotificationSound")
            } else {
                Blank
            });
            v.extend([
                Opt("alwaysShowUpdates"),
                Opt("updateType"),
                Opt("showServerPluginMessage"),
                Opt("showServerPluginMissingMessageAlways"),
                Opt("showChatMessageStencil"),
                Opt("showServerVrChangesMessage"),
            ]);
            v
        }
        PageId::Rendering => vec![
            Opt("renderScaleFactor"),
            Opt("horizonRefreshRate"),
            Opt("horizonDynamicResolution"),
            Opt("horizonOpaqueWater"),
            Opt("horizonVrProfile"),
            Opt("horizonFoveation"),
            Opt("horizonMultiview"),
            Opt("horizonFoveatedMultiview"),
            Opt("horizonVisibilityMask"),
            Opt("horizonVrUploadMs"),
            Opt("horizonFlatDraws"),
            Opt("horizonPinBigCores"),
            Entry::Page(PageId::Experiments),
            Opt("useFsaa"),
            Opt("stencilOn"),
            Entry::Page(PageId::PostEffects),
        ],
        PageId::PostEffects => vec![
            Opt("low_health_indicator"),
            Opt("hitIndicator"),
            Opt("waterEffect"),
            Opt("portalEffect"),
            Opt("freezeEffect"),
            Opt("pumpkinEffect"),
            Opt("sleepEffect"),
        ],
        PageId::Standing => vec![
            Opt("walkUpBlocks"),
            Opt("vehicleRotation"),
            Opt("walkMultiplier"),
            Opt("vrWorldRotationIncrement"),
            Opt("bcbOn"),
            Opt("feetBodyPosition"),
            Opt("worldRotationXSensitivity"),
            Opt("allowStandingOriginOffset"),
            Opt("forceStandingFreeMove"),
            Blank,
            Entry::Page(PageId::Teleport),
            Entry::Page(PageId::FreeMove),
        ],
        PageId::Roomscale => vec![
            Entry::Page(PageId::WeaponCollision),
            Entry::Page(PageId::Backpack),
            Entry::Page(PageId::Climbing),
            Opt("realisticJumpEnabled"),
            Opt("realisticSneakEnabled"),
            Opt("realisticRowEnabled"),
            Opt("realisticSwimEnabled"),
            Opt("bowMode"),
            Opt("allowCrawling"),
            Opt("realisticDismountEnabled"),
            Opt("realisticBlockInteractEnabled"),
            Opt("realisticEntityInteractEnabled"),
            Opt("horizonVivecraftRoomWalk"),
        ],
        PageId::WeaponCollision => vec![
            Opt("weaponCollisionNew"),
            Opt("feetCollision"),
            Opt("blockCollision"),
            Opt("entityCollision"),
            Opt("doorHitting"),
            Opt("swordBlockCollision"),
            Opt("onlySwordCollision"),
            Opt("reducedPlayerReach"),
            Opt("allowBreakingClimbable"),
            Opt("roomscaleSpearLunge"),
        ],
        PageId::Backpack => vec![
            Opt("backpackSwitching"),
            Blank,
            Opt("backpackMainHandKeybind"),
            Opt("backpackOffhandKeybind"),
        ],
        PageId::Climbing => vec![
            Opt("realisticClimbEnabled"),
            Opt("climbingAutoGrab"),
            Opt("vanillaClimbing"),
        ],
        PageId::FreeMove => {
            let mut v = if seated {
                vec![Opt("seatedhmd"), Opt("fovReduction"), Opt("inertiaFactor")]
            } else {
                vec![
                    Opt("vrFreeMoveMode"),
                    Opt("vrFreeMoveFlyMode"),
                    Opt("fovReduction"),
                    Opt("inertiaFactor"),
                    Opt("movementSpeedMultiplier"),
                    Opt("sprintMovementSpeedMultiplier"),
                    Opt("autoSprint"),
                    Opt("autoSprintThreshold"),
                    Opt("analogMovement"),
                    Opt("digitalMovementDeadzone"),
                ]
            };
            if s.bool("fovReduction") {
                if v.len() % 2 == 1 {
                    v.push(Blank);
                }
                v.push(Opt("fovReductionMin"));
                v.push(Opt("fovRedutioncOffset"));
            }
            v
        }
        PageId::Teleport => {
            let mut v = vec![Opt("simulateFalling"), Opt("limitedTeleport")];
            if server_limited_teleport || s.bool("limitedTeleport") {
                v.extend([
                    Opt("teleportLimitUp"),
                    Opt("teleportLimitDown"),
                    Opt("teleportLimitHoriz"),
                ]);
            }
            v
        }
        PageId::Seated => vec![
            Opt("xSensitivity"),
            Opt("ySensitivity"),
            Opt("keyholeX"),
            Opt("seatedHudAltMode"),
            Opt("walkUpBlocks"),
            Opt("vrWorldRotationIncrement"),
            Opt("vehicleRotation"),
            Opt("reverseHands"),
            Opt("seatedFreeMove"),
            Blank,
            Opt("rightclickDelay"),
            Opt("aimDevice"),
            Entry::Page(PageId::FreeMove),
            Entry::Page(PageId::Teleport),
        ],
        PageId::PlayerModel => vec![
            Opt("shouldRenderSelf"),
            Opt("playerModelType"),
            Opt("modelArmsMode"),
            Opt("playerModelArmsScale"),
            Opt("showPlayerHands"),
            Opt("playerModelBodyScale"),
            Opt("playerLimbsConnected"),
            Opt("playerModelLegScale"),
            Opt("playerLimbsLimit"),
            Opt("playerWalkAnim"),
            Opt("playerArmAnim"),
            Opt("applyPlayerWorldscale"),
            Entry::CalibrateHeight,
        ],
        PageId::Keyboard => vec![
            Opt("physicalKeyboard"),
            Opt("keyboardPressBinds"),
            Opt("autoOpenKeyboard"),
            Opt("autoCloseKeyboard"),
            Opt("physicalKeyboardScale"),
            Opt("physicalKeyboardTheme"),
            Opt("keyboardShowLayoutSelect"),
        ],
        PageId::Radial => vec![
            Opt("radialModeHold"),
            Opt("radialRepeat"),
            Opt("vrRadialButtons"),
        ],
        PageId::MenuWorld => vec![
            Opt("menuWorldSelection"),
            Blank,
            Opt("menuWorldFallbackPanorama"),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::info;

    #[test]
    fn every_listed_option_exists() {
        let mut s = VrSettings::default();
        s.set_bool("fovReduction", true);
        s.set("chatNotifications", "BOTH");
        for seated in [false, true] {
            s.set_bool("seated", seated);
            for page in [
                PageId::Main,
                PageId::Hud,
                PageId::OtherHud,
                PageId::Rendering,
                PageId::PostEffects,
                PageId::Standing,
                PageId::Roomscale,
                PageId::WeaponCollision,
                PageId::Backpack,
                PageId::Climbing,
                PageId::FreeMove,
                PageId::Teleport,
                PageId::Seated,
                PageId::PlayerModel,
                PageId::Keyboard,
                PageId::Radial,
                PageId::MenuWorld,
                PageId::Visor,
                PageId::Experiments,
                PageId::Presets,
                PageId::VRendering,
                PageId::Effects,
                PageId::EyeEffects,
                PageId::VBody,
                PageId::BodySelect,
                PageId::Height,
                PageId::Controls,
                PageId::Bindings,
                PageId::Movement,
                PageId::Immersion,
                PageId::ImmersionAdvanced,
                PageId::Gui,
                PageId::Hotbar,
                PageId::World,
            ] {
                for e in entries(page, &s, true) {
                    if let Entry::Opt(k) = e {
                        assert!(info(k).is_some(), "{page:?}: {k}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_main_page_lists_visors_categories_in_its_order() {
        let s = VrSettings::horizon();
        let pages: Vec<_> = entries(PageId::Main, &s, false)
            .into_iter()
            .filter_map(|e| match e {
                Entry::Page(p) => Some(p),
                _ => None,
            })
            .collect();
        // `VRSettingsCategory`: PRESETS, RENDERING, VR_BODY, CONTROLS, MOVEMENT, IMMERSION, GUI, WORLD.
        assert_eq!(
            pages,
            [
                PageId::Presets,
                PageId::VRendering,
                PageId::VBody,
                PageId::Controls,
                PageId::Movement,
                PageId::Immersion,
                PageId::Gui,
                PageId::World,
            ]
        );
        // None of Vivecraft's pages is reachable from it.
        for page in &pages {
            assert!(page.title().starts_with("visor."), "{page:?}");
        }
    }
}
