// SPDX-License-Identifier: LGPL-3.0-only
// Copyright (c) 2025-2026 Phoenix-Ra and VisorEssentials contributors.
//! EssentialsChannel, BowTensionPayloadToServer and BowDrawCancelPayloadToServer.
pub const CHANNEL: &str = "visor_essentials:channel";
pub const VERSION: i32 = 2;
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Payload {
    Tension(f32),
    Cancel,
}
impl Payload {
    pub fn write(self) -> Vec<u8> {
        match self {
            Self::Cancel => vec![1],
            Self::Tension(t) => {
                let mut b = vec![0];
                b.extend(t.to_be_bytes());
                b
            }
        }
    }
    pub fn read(bytes: &[u8]) -> Option<Self> {
        match bytes {
            [1] => Some(Self::Cancel),
            [0, a, b, c, d] => {
                let f = f32::from_be_bytes([*a, *b, *c, *d]);
                (f.is_finite() && (0.0..=1.0).contains(&f)).then_some(Self::Tension(f))
            }
            _ => None,
        }
    }
}
pub fn settings(bytes: &[u8]) -> Option<bool> {
    let (&0, data) = bytes.split_first()? else {
        return None;
    };
    let text = std::str::from_utf8(data).ok()?;
    text.lines()
        .filter_map(|l| l.split('#').next()?.trim().split_once(':'))
        .find_map(|(k, v)| {
            if k.trim() != "betterBow" {
                return None;
            }
            match v.trim() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            }
        })
}
pub fn settings_payload(better_bow: bool) -> Vec<u8> {
    let mut b = vec![0];
    b.extend(format!("betterBow: {better_bow}\n").bytes());
    b
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn java_wire_fixtures() {
        assert_eq!(Payload::Tension(0.5).write(), [0, 0x3f, 0, 0, 0]);
        assert_eq!(
            Payload::read(&[0, 0x3f, 0x80, 0, 0]),
            Some(Payload::Tension(1.0))
        );
        assert_eq!(Payload::Cancel.write(), [1]);
        assert_eq!(settings(b"\0betterBow: false\n"), Some(false));
    }
    #[test]
    fn malformed_and_non_finite_draws_are_rejected() {
        for b in [
            &[0, 0x7f, 0xc0, 0, 0][..],
            &[0, 0x3f, 0x80, 0, 1],
            &[0],
            &[1, 0],
            &[2],
        ] {
            assert_eq!(Payload::read(b), None);
        }
        assert_eq!(settings(b"\0betterBow: maybe"), None);
    }
}
