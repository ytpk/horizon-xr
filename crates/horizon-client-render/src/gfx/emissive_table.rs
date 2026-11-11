//! The emissive overlay of each block sprite (`native_emissive`'s pairs), one texel per sprite
//! layer as terrain.wgsl's `emissive_table` reads it: x the layer in its array, y the array,
//! the overlay's layer or `NONE`. The vertex shader takes it with one load; the search through
//! `Globals.emissive_layers` it replaces cost Adreno (Quest) a tenth of the frame at 24 chunks
//! just by being in the shader, even with no pairs.

/// No overlay.
pub const NONE: u32 = u32::MAX;

pub struct EmissiveTable {
    texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    split: u32,
    arrays: u32,
    /// The pairs the texture holds now.
    held: Vec<[u32; 4]>,
}

impl EmissiveTable {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, split: u32, arrays: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Horizon emissive overlays"),
            size: wgpu::Extent3d {
                width: split.max(1),
                height: arrays.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let table = Self {
            texture,
            view,
            split: split.max(1),
            arrays: arrays.max(1),
            held: Vec::new(),
        };
        table.write(queue, &texels(&[], table.split, table.arrays));
        table
    }

    /// Holds `pairs` (`[base layer, overlay layer, _, _]`, sorted by base layer) from now on.
    pub fn update(&mut self, queue: &wgpu::Queue, pairs: &[[u32; 4]]) {
        if self.held == pairs {
            return;
        }
        self.write(queue, &texels(pairs, self.split, self.arrays));
        self.held = pairs.to_vec();
    }

    fn write(&self, queue: &wgpu::Queue, texels: &[u32]) {
        queue.write_texture(
            self.texture.as_image_copy(),
            bytemuck::cast_slice(texels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.split * 4),
                rows_per_image: None,
            },
            self.texture.size(),
        );
    }
}

/// Every sprite's overlay, `split` layers a row and `arrays` rows: the first pair of its
/// layer, as the search took it.
fn texels(pairs: &[[u32; 4]], split: u32, arrays: u32) -> Vec<u32> {
    let mut texels = vec![NONE; (split * arrays) as usize];
    for pair in pairs {
        if let Some(texel) = texels.get_mut(pair[0] as usize)
            && *texel == NONE
        {
            *texel = pair[1];
        }
    }
    texels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sprite_takes_the_first_overlay_of_its_layer() {
        // Split 4, two arrays: layer 5 is texel (1, 1) of the 4x2 table, index 5.
        let t = texels(&[[5, 40, 0, 0], [5, 41, 0, 0], [2, 9, 0, 0]], 4, 2);
        assert_eq!(t.len(), 8);
        assert_eq!(t[5], 40);
        assert_eq!(t[2], 9);
        assert_eq!(t.iter().filter(|&&x| x != NONE).count(), 2);
    }

    #[test]
    fn a_pair_past_the_table_is_left_out() {
        let t = texels(&[[8, 1, 0, 0]], 4, 2);
        assert!(t.iter().all(|&x| x == NONE));
    }
}
