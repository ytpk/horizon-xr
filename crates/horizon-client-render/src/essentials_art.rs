// SPDX-License-Identifier: LGPL-3.0-only
// Copyright (c) 2025-2026 Phoenix-Ra and VisorEssentials contributors.
//! Unmodified VisorEssentials GUI textures, source 88ffec507449eb1a97ccbf495cd331a46c6d635b.
pub fn images() -> impl Iterator<Item = (String, u32, u32, Vec<u8>)> {
    const ART: &[(&str, &[u8])] = &[
        (
            "horizon/essentials/container/anvil",
            include_bytes!("../assets/essentials/container/anvil.png"),
        ),
        (
            "horizon/essentials/container/beacon",
            include_bytes!("../assets/essentials/container/beacon.png"),
        ),
        (
            "horizon/essentials/container/blast_furnace",
            include_bytes!("../assets/essentials/container/blast_furnace.png"),
        ),
        (
            "horizon/essentials/container/brewing_stand",
            include_bytes!("../assets/essentials/container/brewing_stand.png"),
        ),
        (
            "horizon/essentials/container/cartography_table",
            include_bytes!("../assets/essentials/container/cartography_table.png"),
        ),
        (
            "horizon/essentials/container/crafting_table",
            include_bytes!("../assets/essentials/container/crafting_table.png"),
        ),
        (
            "horizon/essentials/container/dispenser",
            include_bytes!("../assets/essentials/container/dispenser.png"),
        ),
        (
            "horizon/essentials/container/enchanting_table",
            include_bytes!("../assets/essentials/container/enchanting_table.png"),
        ),
        (
            "horizon/essentials/container/furnace",
            include_bytes!("../assets/essentials/container/furnace.png"),
        ),
        (
            "horizon/essentials/container/generic_54",
            include_bytes!("../assets/essentials/container/generic_54.png"),
        ),
        (
            "horizon/essentials/container/grindstone",
            include_bytes!("../assets/essentials/container/grindstone.png"),
        ),
        (
            "horizon/essentials/container/hopper",
            include_bytes!("../assets/essentials/container/hopper.png"),
        ),
        (
            "horizon/essentials/container/horse",
            include_bytes!("../assets/essentials/container/horse.png"),
        ),
        (
            "horizon/essentials/container/loom",
            include_bytes!("../assets/essentials/container/loom.png"),
        ),
        (
            "horizon/essentials/container/shulker_box",
            include_bytes!("../assets/essentials/container/shulker_box.png"),
        ),
        (
            "horizon/essentials/container/smithing",
            include_bytes!("../assets/essentials/container/smithing.png"),
        ),
        (
            "horizon/essentials/container/smoker",
            include_bytes!("../assets/essentials/container/smoker.png"),
        ),
        (
            "horizon/essentials/container/stonecutter",
            include_bytes!("../assets/essentials/container/stonecutter.png"),
        ),
        (
            "horizon/essentials/container/villager",
            include_bytes!("../assets/essentials/container/villager.png"),
        ),
        (
            "horizon/essentials/inventory",
            include_bytes!("../assets/essentials/inventory.png"),
        ),
        (
            "horizon/essentials/inventory_simplified",
            include_bytes!("../assets/essentials/inventory_simplified.png"),
        ),
        (
            "horizon/essentials/inventory_with_offhand",
            include_bytes!("../assets/essentials/inventory_with_offhand.png"),
        ),
    ];
    ART.iter().filter_map(|(name, bytes)| {
        let (w, h, pixels) = crate::visor_art::decode(bytes)?;
        Some(((*name).to_string(), w, h, pixels))
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_upstream_inventory_and_container_textures_decode() {
        let images: Vec<_> = super::images().collect();
        assert_eq!(images.len(), 22);
        for (name, w, h, pixels) in &images {
            assert_eq!(pixels.len(), (w * h * 4) as usize, "{name}");
        }
        let size = |suffix: &str| {
            let (_, w, h, _) = images.iter().find(|(n, ..)| n.ends_with(suffix)).unwrap();
            (*w, *h)
        };
        assert_eq!(size("/inventory_with_offhand"), (278, 156));
        assert_eq!(size("/inventory"), (258, 156));
        assert_eq!(size("/container/villager"), (512, 256));
    }
}
