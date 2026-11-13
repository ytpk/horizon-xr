// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports `VRSettings.ServerOverrides` and the `SETTING_OVERRIDE` handling of
//! `ClientNetworking`: settings a Vivecraft server forces (teleport limits, world scale
//! range, third person items), applied over the player's own without changing them.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).

use crate::settings::{Kind, VrSettings, def};
use std::collections::BTreeMap;

/// The settings a server may override, by network name (the config key).
pub const OVERRIDABLE: [&str; 7] = [
    "limitedTeleport",
    "teleportLimitUp",
    "teleportLimitDown",
    "teleportLimitHoriz",
    "worldScale",
    "thirdPersonItems",
    "thirdPersonItemsCustom",
];

#[derive(Clone, Debug, Default, PartialEq)]
struct Setting {
    value: Option<String>,
    min: Option<f32>,
    max: Option<f32>,
}

/// What the server forced.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerOverrides {
    map: BTreeMap<&'static str, Setting>,
}

fn is_float(key: &str) -> bool {
    def(key).is_some_and(|d| matches!(d.kind, Kind::Float | Kind::Int) && d.range.is_some())
}

impl ServerOverrides {
    /// `resetAll`.
    pub fn reset_all(&mut self) {
        self.map.clear();
    }

    /// The `SETTING_OVERRIDE` payload: `name`, `name.min` and `name.max` entries.
    pub fn apply(&mut self, overrides: &[(String, String)], clear: bool) {
        for (name, value) in overrides {
            let (key, part) = match name.split_once('.') {
                Some((k, p)) => (k, Some(p)),
                None => (name.as_str(), None),
            };
            let Some(key) = OVERRIDABLE.iter().find(|k| **k == key) else {
                continue;
            };
            if clear {
                self.map.remove(key);
                continue;
            }
            let entry = self.map.entry(key).or_default();
            match part {
                Some("min") => entry.min = value.parse().ok().or(entry.min),
                Some("max") => entry.max = value.parse().ok().or(entry.max),
                Some(_) => {}
                None => entry.value = Some(value.clone()),
            }
        }
    }

    /// The server's `(min, max)` for a float option, with the missing end open.
    pub fn range(&self, key: &str) -> Option<(f32, f32)> {
        let s = self.map.get(key)?;
        (s.min.is_some() || s.max.is_some())
            .then(|| (s.min.unwrap_or(f32::MIN), s.max.unwrap_or(f32::MAX)))
    }

    pub fn is_overridden(&self, key: &str) -> bool {
        self.map
            .get(key)
            .is_some_and(|s| s.value.is_some() || s.min.is_some() || s.max.is_some())
    }

    /// The player's settings with the server's values and limits laid over them
    /// (`Setting.getValue`: the override, clamped to the server's range).
    pub fn effective(&self, own: &VrSettings) -> VrSettings {
        let mut out = own.clone();
        for (key, s) in &self.map {
            let value = s
                .value
                .clone()
                .or_else(|| is_float(key).then(|| own.raw_value(key)));
            let Some(value) = value else { continue };
            if is_float(key) {
                let v: f32 = value.parse().unwrap_or(0.0);
                let v = v.clamp(s.min.unwrap_or(f32::MIN), s.max.unwrap_or(f32::MAX));
                if matches!(def(key).map(|d| d.kind), Some(Kind::Int)) {
                    out.set_int(key, v.round() as i32);
                } else {
                    out.set_float(key, v);
                }
            } else {
                out.set(key, &value);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn a_server_forces_values_and_limits_ranges() {
        let own = VrSettings::default();
        let mut o = ServerOverrides::default();
        o.apply(
            &pairs(&[
                ("limitedTeleport", "true"),
                ("teleportLimitUp", "2"),
                ("worldScale.max", "2.0"),
                ("nope", "1"),
            ]),
            false,
        );
        let s = o.effective(&own);
        assert!(s.bool("limitedTeleport"));
        assert_eq!(s.int("teleportLimitUp"), 2);
        assert!(o.is_overridden("worldScale"));
        assert_eq!(o.range("worldScale"), Some((f32::MIN, 2.0)));
        // The player's own world scale is clamped to the server's maximum.
        let mut big = VrSettings::default();
        big.set_float("worldScale", 8.0);
        assert_eq!(o.effective(&big).float("worldScale"), 2.0);
        assert_eq!(o.effective(&own).float("worldScale"), 1.0);
        assert!(!o.is_overridden("nope"));
    }

    #[test]
    fn clearing_gives_the_player_their_settings_back() {
        let own = VrSettings::default();
        let mut o = ServerOverrides::default();
        o.apply(&pairs(&[("teleportLimitHoriz", "3")]), false);
        assert_eq!(o.effective(&own).int("teleportLimitHoriz"), 3);
        o.apply(&pairs(&[("teleportLimitHoriz", "")]), true);
        assert_eq!(o.effective(&own), own);
        o.apply(&pairs(&[("teleportLimitHoriz", "3")]), false);
        o.reset_all();
        assert_eq!(o.effective(&own), own);
    }
}
