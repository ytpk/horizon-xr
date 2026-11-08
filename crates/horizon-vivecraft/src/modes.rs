// SPDX-License-Identifier: LGPL-3.0-or-later
//! Typed views of the enum settings of `org.vivecraft.client_vr.settings.VRSettings`
//! (`HUDLock`, `FreeMove`, `RealisticJump`, `WeaponCollision`, `InertiaFactor`, `BowMode`...)
//! and the helpers on `VRSettings` that read them (`getVrFreeMoveMode`).
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).

use crate::settings::VrSettings;

/// `VRSettings.HUDLock`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudLock {
    Wrist,
    Hand,
    Head,
}

/// `VRSettings.FreeMove`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreeMove {
    Controller,
    Hmd,
    RunInPlace,
    Room,
    Waist,
    /// Only for flying: use the walking mode.
    Auto,
}

/// `RealisticJump` and `WeaponCollision`: off, on, or decided by the game mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tri {
    Off,
    On,
    Auto,
}

/// `VRSettings.BowMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BowMode {
    Off,
    Vanilla,
    On,
}

/// `VRSettings.RenderPointerElement`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerElement {
    Always,
    WithHud,
    Never,
}

fn tri(name: &str) -> Tri {
    match name {
        "ON" => Tri::On,
        "AUTO" => Tri::Auto,
        _ => Tri::Off,
    }
}

impl VrSettings {
    pub fn seated(&self) -> bool {
        self.bool("seated")
    }
    /// `reverseHands`: the left controller is the main hand.
    pub fn left_handed(&self) -> bool {
        self.bool("reverseHands")
    }
    pub fn hud_lock(&self) -> HudLock {
        match self.enum_name("vrHudLockMode") {
            "HAND" => HudLock::Hand,
            "HEAD" => HudLock::Head,
            _ => HudLock::Wrist,
        }
    }
    fn free_move_of(&self, key: &str) -> FreeMove {
        match self.enum_name(key) {
            "HMD" => FreeMove::Hmd,
            "RUN_IN_PLACE" => FreeMove::RunInPlace,
            "ROOM" => FreeMove::Room,
            "WAIST" => FreeMove::Waist,
            "AUTO" => FreeMove::Auto,
            _ => FreeMove::Controller,
        }
    }
    /// `VRSettings.getVrFreeMoveMode`: the mode for walking, or for flying and swimming.
    pub fn free_move_mode(&self, fly_swimming: bool) -> FreeMove {
        let fly = self.free_move_of("vrFreeMoveFlyMode");
        if fly_swimming && fly != FreeMove::Auto {
            fly
        } else {
            self.free_move_of("vrFreeMoveMode")
        }
    }
    pub fn realistic_jump(&self) -> Tri {
        tri(self.enum_name("realisticJumpEnabled"))
    }
    pub fn weapon_collision(&self) -> Tri {
        tri(self.enum_name("weaponCollisionNew"))
    }
    pub fn bow_mode(&self) -> BowMode {
        match self.enum_name("bowMode") {
            "OFF" => BowMode::Off,
            "VANILLA" => BowMode::Vanilla,
            _ => BowMode::On,
        }
    }
    /// `InertiaFactor.getFactor`.
    pub fn inertia_factor(&self) -> f32 {
        match self.enum_name("inertiaFactor") {
            "NONE" => 1.0 / 0.01,
            "LARGE" => 0.25,
            "MASSIVE" => 1.0 / 16.0,
            _ => 1.0,
        }
    }
    pub fn crosshair_mode(&self) -> PointerElement {
        pointer(self.enum_name("renderInGameCrosshairMode"))
    }
    pub fn block_outline_mode(&self) -> PointerElement {
        pointer(self.enum_name("renderBlockOutlineMode"))
    }
    /// The aim device is the head, not the controller.
    pub fn aim_with_hmd(&self) -> bool {
        self.enum_name("aimDevice") == "HMD"
    }
}

fn pointer(name: &str) -> PointerElement {
    match name {
        "WITH_HUD" => PointerElement::WithHud,
        "NEVER" => PointerElement::Never,
        _ => PointerElement::Always,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_views_read_the_enum_settings() {
        let mut s = VrSettings::default();
        assert_eq!(s.hud_lock(), HudLock::Wrist);
        assert_eq!(s.free_move_mode(false), FreeMove::Controller);
        // The default fly mode is AUTO: flying uses the walking mode.
        assert_eq!(s.free_move_mode(true), FreeMove::Controller);
        s.set("vrFreeMoveFlyMode", "HMD");
        assert_eq!(s.free_move_mode(true), FreeMove::Hmd);
        assert_eq!(s.free_move_mode(false), FreeMove::Controller);
        s.set("inertiaFactor", "LARGE");
        assert_eq!(s.inertia_factor(), 0.25);
        assert_eq!(s.bow_mode(), BowMode::On);
    }
}
