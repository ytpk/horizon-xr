// SPDX-License-Identifier: LGPL-3.0-or-later
//! The part of Minecraft's `FriendlyByteBuf` that Vivecraft's payloads use.
//!
//! Part of the Horizon port of Vivecraft (https://github.com/Vivecraft/VivecraftMod,
//! commit 8b4d6b95a1891d9839f19b45c13e93415c02a31e). Ports the buffer calls made by
//! `org.vivecraft.common.network.*`.
//! Copyright (c) Vivecraft contributors. Licensed under the GNU Lesser General Public
//! License, version 3 or (at your option) any later version; see LICENSE-LGPL-3.0.

use crate::{Error, Result};

/// A write buffer; all numbers are big endian, as `FriendlyByteBuf`'s are.
#[derive(Default, Clone, Debug)]
pub struct Writer {
    pub data: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn byte(&mut self, v: u8) {
        self.data.push(v);
    }
    pub fn bool(&mut self, v: bool) {
        self.data.push(u8::from(v));
    }
    pub fn float(&mut self, v: f32) {
        self.data.extend_from_slice(&v.to_be_bytes());
    }
    /// `writeInt`.
    pub fn int(&mut self, v: i32) {
        self.data.extend_from_slice(&v.to_be_bytes());
    }
    pub fn long(&mut self, v: i64) {
        self.data.extend_from_slice(&v.to_be_bytes());
    }
    pub fn bytes(&mut self, v: &[u8]) {
        self.data.extend_from_slice(v);
    }
    pub fn var_int(&mut self, mut v: u32) {
        while v & !0x7F != 0 {
            self.data.push(((v & 0x7F) | 0x80) as u8);
            v >>= 7;
        }
        self.data.push(v as u8);
    }
    /// `writeUtf`: a VarInt of the UTF-8 byte length, then the bytes.
    pub fn utf(&mut self, s: &str) {
        self.var_int(s.len() as u32);
        self.data.extend_from_slice(s.as_bytes());
    }
    /// `writeUUID`: the most significant long, then the least significant.
    pub fn uuid(&mut self, v: u128) {
        self.data.extend_from_slice(&v.to_be_bytes());
    }
}

/// A read buffer.
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    /// `readableBytes`.
    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.remaining() < n {
            return Err(Error::new("vivecraft payload: truncated"));
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    /// `readByte` (signed).
    pub fn byte(&mut self) -> Result<i8> {
        Ok(self.take(1)?[0] as i8)
    }
    /// `readUnsignedByte`.
    pub fn ubyte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn bool(&mut self) -> Result<bool> {
        Ok(self.take(1)?[0] != 0)
    }
    /// `readInt`.
    pub fn int(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    /// `readLong`.
    pub fn long(&mut self) -> Result<i64> {
        Ok(i64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub fn float(&mut self) -> Result<f32> {
        Ok(f32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn var_int(&mut self) -> Result<u32> {
        let mut v = 0u32;
        for i in 0..5 {
            let b = self.take(1)?[0];
            v |= u32::from(b & 0x7F) << (7 * i);
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(Error::new("vivecraft payload: VarInt too big"))
    }
    /// `readUtf` (cap 32767 characters, as the default).
    pub fn utf(&mut self) -> Result<String> {
        let n = self.var_int()? as usize;
        if n > 32767 * 3 {
            return Err(Error::new("vivecraft payload: string too long"));
        }
        let b = self.take(n)?;
        String::from_utf8(b.to_vec()).map_err(|_| Error::new("vivecraft payload: bad UTF-8"))
    }
    pub fn uuid(&mut self) -> Result<u128> {
        Ok(u128::from_be_bytes(self.take(16)?.try_into().unwrap()))
    }
    /// Everything left.
    pub fn rest(&mut self) -> &'a [u8] {
        let s = &self.data[self.pos..];
        self.pos = self.data.len();
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf_is_var_int_prefixed() {
        let mut w = Writer::new();
        w.utf("vivecraft:data");
        assert_eq!(w.data[0], 14);
        assert_eq!(&w.data[1..], b"vivecraft:data");
        assert_eq!(Reader::new(&w.data).utf().unwrap(), "vivecraft:data");
    }

    #[test]
    fn var_int_matches_minecraft_vectors() {
        // Values from the protocol documentation's VarInt table.
        for (v, bytes) in [
            (0u32, vec![0x00u8]),
            (127, vec![0x7f]),
            (128, vec![0x80, 0x01]),
            (255, vec![0xff, 0x01]),
            (25565, vec![0xdd, 0xc7, 0x01]),
            (2097151, vec![0xff, 0xff, 0x7f]),
        ] {
            let mut w = Writer::new();
            w.var_int(v);
            assert_eq!(w.data, bytes, "{v}");
            assert_eq!(Reader::new(&bytes).var_int().unwrap(), v);
        }
    }

    #[test]
    fn floats_are_big_endian_and_uuid_msb_first() {
        let mut w = Writer::new();
        w.float(1.0);
        assert_eq!(w.data, [0x3f, 0x80, 0, 0]);
        let mut w = Writer::new();
        w.uuid(0x0011_2233_4455_6677_8899_aabb_ccdd_eeff);
        assert_eq!(w.data[0], 0x00);
        assert_eq!(w.data[15], 0xff);
    }

    #[test]
    fn short_reads_fail() {
        assert!(Reader::new(&[1, 2]).float().is_err());
        assert!(Reader::new(&[5, b'a']).utf().is_err());
    }
}
