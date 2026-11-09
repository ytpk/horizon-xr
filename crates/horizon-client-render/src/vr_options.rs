// SPDX-License-Identifier: LGPL-3.0-or-later
//! The VR options pages' options (`Opt::Vr`): Vivecraft's `VrOptions` over the
//! `vivecraft-client-config.json` table (`Settings::vr`), shown with Vivecraft's names, values
//! and ranges. The page structure, the display strings and the slider mappings are
//! `horizon_vivecraft::{pages, options}` (ports of `org.vivecraft.client.gui.settings.*` and
//! `VRSettings.VrOptions`). The pages show only while a headset is installed.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).
use crate::settings::Settings;
use crate::text::Text;
use horizon_vivecraft::options::{self, OPTS, OptInfo, Shown};
use std::sync::Arc;

/// An option of the table: its index in [`OPTS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VrOpt(u16);

impl VrOpt {
    /// The option editing a config key.
    pub fn by_key(key: &str) -> Option<Self> {
        OPTS.iter()
            .position(|o| o.key == key)
            .map(|i| Self(i as u16))
    }

    pub fn by_name(name: &str) -> Option<Self> {
        OPTS.iter()
            .position(|o| o.name == name)
            .map(|i| Self(i as u16))
    }

    fn info(self) -> &'static OptInfo {
        &OPTS[usize::from(self.0)]
    }

    pub fn caption(self) -> &'static str {
        self.info().lang
    }

    /// The slider's position (0..1), or `None` for a button that cycles.
    pub fn slider(self, s: &Settings) -> Option<f32> {
        options::slider(&s.vr, self.info()).map(|(at, _)| at)
    }

    pub fn steps(self) -> Option<u32> {
        // The slider's detents are Vivecraft's step over the range.
        Some(options::slider(&Default::default(), self.info())?.1)
    }

    pub fn set_slider(self, s: &mut Settings, at: f32) {
        options::set_slider(Arc::make_mut(&mut s.vr), self.info(), at, None);
    }

    pub fn cycle(self, s: &mut Settings, back: bool) {
        options::cycle(Arc::make_mut(&mut s.vr), self.info(), back);
    }

    /// The button's text: its caption and value.
    pub fn label(self, s: &Settings) -> Text {
        let tr = |key: &str| Text::translate(key, &[]);
        let value = match options::shown(&s.vr, self.info()) {
            Shown::Literal(text) => Text::literal(text),
            Shown::Lang(key) => tr(&key),
            Shown::LangArg(key, arg) => Text::translate(&key, &[Text::literal(arg)]),
        };
        let value = if options::binding_conflict(&s.vr, self.info().key) {
            Text::translate("horizon.vr.binding.shared", &[value])
        } else {
            value
        };
        Text::translate("options.generic_value", &[tr(self.caption()), value])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sliders_and_cycles_edit_the_table() {
        let mut s = Settings::default();
        let dist = VrOpt::by_key("hudDistance").unwrap();
        dist.set_slider(&mut s, 1.0);
        assert!((s.vr.float("hudDistance") - 5.0).abs() < 1e-4);
        assert_eq!(dist.slider(&s), Some(1.0));
        let lock = VrOpt::by_key("vrHudLockMode").unwrap();
        assert!(lock.slider(&s).is_none());
        // Visor's default is the head; the next constant after HEAD wraps to WRIST.
        assert_eq!(s.vr.enum_name("vrHudLockMode"), "HEAD");
        lock.cycle(&mut s, false);
        assert_eq!(s.vr.enum_name("vrHudLockMode"), "WRIST");
        let rot = VrOpt::by_key("vrWorldRotationIncrement").unwrap();
        assert_eq!(rot.steps(), Some(5));
    }
}
