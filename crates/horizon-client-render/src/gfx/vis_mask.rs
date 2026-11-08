//! The headset's visibility mask (`XR_KHR_visibility_mask`): the part of each eye's image the
//! lens never shows. Its triangles are drawn first in the eye's world pass, depth only and at the
//! nearest depth, so the depth test throws away what the pass would draw behind them before the
//! fragments are shaded. The colour there stays the clear colour; nobody sees it.
//!
//! The runtime gives the mesh in the lens's tangent space; some runtimes are said to give
//! normalized device coordinates. [`Space`] is told from the mesh's own extent against the eye's
//! field of view, and a mesh that fits neither is not drawn.
use super::headset::HiddenArea;

/// The coordinates a mesh is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    /// `x`, `y` are tangents of the view angles (left and down negative).
    Tangent,
    /// -1..1 across the image.
    Ndc,
}

/// How near the mesh's extent must be to the field of view's (a share of its size).
const FIT: f32 = 0.03;

/// Which space `vertices` are in, going by where their extent reaches: a hidden-area mesh covers
/// the image's corners, so it touches the field of view's rectangle on every side. `None` when
/// it fits neither space, or has no vertices.
pub fn space_of(vertices: &[[f32; 2]], tangents: [f32; 4]) -> Option<Space> {
    let [left, right, up, down] = tangents;
    let (mut min, mut max) = ([f32::MAX; 2], [f32::MIN; 2]);
    for v in vertices {
        for i in 0..2 {
            min[i] = min[i].min(v[i]);
            max[i] = max[i].max(v[i]);
        }
    }
    if vertices.is_empty() || !min.iter().chain(&max).all(|v| v.is_finite()) {
        return None;
    }
    let near = |value: f32, target: f32, size: f32| (value - target).abs() <= FIT * size;
    let fits = |lo: [f32; 2], hi: [f32; 2]| {
        near(min[0], lo[0], hi[0] - lo[0])
            && near(max[0], hi[0], hi[0] - lo[0])
            && near(min[1], lo[1], hi[1] - lo[1])
            && near(max[1], hi[1], hi[1] - lo[1])
    };
    if fits([left, down], [right, up]) {
        Some(Space::Tangent)
    } else if fits([-1.0, -1.0], [1.0, 1.0]) {
        Some(Space::Ndc)
    } else {
        None
    }
}

/// The mesh's vertices as normalized device coordinates for an eye of `tangents`.
pub fn to_ndc(vertices: &[[f32; 2]], tangents: [f32; 4], space: Space) -> Vec<[f32; 2]> {
    match space {
        Space::Ndc => vertices.to_vec(),
        Space::Tangent => {
            let [left, right, up, down] = tangents;
            vertices
                .iter()
                .map(|v| {
                    [
                        2.0 * (v[0] - left) / (right - left) - 1.0,
                        2.0 * (v[1] - down) / (up - down) - 1.0,
                    ]
                })
                .collect()
        }
    }
}

/// An eye's mask ready to draw: normalized device coordinates and triangle indices.
pub struct EyeMask {
    pub vertices: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

impl EyeMask {
    /// `None` for a mesh that is empty, has an index out of range, or fits no space.
    pub fn new(area: &HiddenArea, tangents: [f32; 4]) -> Option<Self> {
        let space = space_of(&area.vertices, tangents)?;
        let count = area.vertices.len() as u32;
        if area.indices.is_empty()
            || !area.indices.len().is_multiple_of(3)
            || area.indices.iter().any(|&i| i >= count)
        {
            return None;
        }
        Some(Self {
            vertices: to_ndc(&area.vertices, tangents, space),
            indices: area.indices.clone(),
        })
    }
}

/// The pipeline and buffers that draw an eye's mask.
pub(super) struct VisMask {
    module: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
    pipelines: Vec<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    /// Bytes the two buffers hold.
    capacity: [u64; 2],
    count: u32,
}

impl VisMask {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        Self {
            module: device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Visibility mask"),
                source: wgpu::ShaderSource::Wgsl(include_str!("vis_mask.wgsl").into()),
            }),
            layout: device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Visibility mask"),
                bind_group_layouts: &[],
                ..Default::default()
            }),
            pipelines: Vec::new(),
            vertices: Self::buffer(device, 4096, wgpu::BufferUsages::VERTEX),
            indices: Self::buffer(device, 4096, wgpu::BufferUsages::INDEX),
            capacity: [4096; 2],
            count: 0,
        }
    }

    fn buffer(device: &wgpu::Device, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Visibility mask"),
            size,
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Puts `mask` in the buffers (in the encoder's order) and makes the pipeline for a colour
    /// target of `format`.
    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        uploads: &mut super::upload::Uploads,
        encoder: &mut wgpu::CommandEncoder,
        mask: &EyeMask,
        format: wgpu::TextureFormat,
    ) {
        let vertices: &[u8] = bytemuck::cast_slice(&mask.vertices);
        let indices: &[u8] = bytemuck::cast_slice(&mask.indices);
        if vertices.len() as u64 > self.capacity[0] {
            self.capacity[0] = (vertices.len() as u64).next_power_of_two();
            self.vertices = Self::buffer(device, self.capacity[0], wgpu::BufferUsages::VERTEX);
        }
        if indices.len() as u64 > self.capacity[1] {
            self.capacity[1] = (indices.len() as u64).next_power_of_two();
            self.indices = Self::buffer(device, self.capacity[1], wgpu::BufferUsages::INDEX);
        }
        uploads.buffer(encoder, &self.vertices, 0, vertices);
        uploads.buffer(encoder, &self.indices, 0, indices);
        self.count = mask.indices.len() as u32;
        if !self.pipelines.iter().any(|(f, _)| *f == format) {
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Visibility mask"),
                layout: Some(&self.layout),
                vertex: wgpu::VertexState {
                    module: &self.module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: 8,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x2],
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &self.module,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::empty(),
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
            self.pipelines.push((format, pipeline));
        }
    }

    /// Draws the mask `prepare` put in, in a pass whose colour target is `format`'s.
    pub(super) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, format: wgpu::TextureFormat) {
        let Some((_, pipeline)) = self.pipelines.iter().find(|(f, _)| *f == format) else {
            return;
        };
        pass.set_pipeline(pipeline);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.count, 0, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four corner triangles of a lens `[-1.2, 0.9, 1.0, -1.1]` wide (tangents).
    fn corners() -> Vec<[f32; 2]> {
        vec![
            [-1.2, -1.1],
            [0.9, -1.1],
            [0.9, 1.0],
            [-1.2, 1.0],
            [-0.6, -0.5],
            [0.3, 0.4],
        ]
    }

    const LENS: [f32; 4] = [-1.2, 0.9, 1.0, -1.1];

    #[test]
    fn a_mesh_reaching_the_fields_corners_is_in_tangent_space() {
        assert_eq!(space_of(&corners(), LENS), Some(Space::Tangent));
    }

    #[test]
    fn a_mesh_reaching_minus_one_to_one_is_normalized_device_coordinates() {
        let ndc = [
            [-1.0, -1.0],
            [1.0, -1.0],
            [1.0, 1.0],
            [-1.0, 1.0],
            [0.0, 0.0],
        ];
        assert_eq!(space_of(&ndc, LENS), Some(Space::Ndc));
    }

    #[test]
    fn a_mesh_that_fits_neither_is_refused() {
        let far = [[-5.0, -5.0], [5.0, 5.0], [0.0, 0.0]];
        assert_eq!(space_of(&far, LENS), None);
        assert_eq!(space_of(&[], LENS), None);
        // Reaching only half the field of view: a corner cut, not a hidden area.
        let small = [[-0.1, -0.1], [0.1, 0.1]];
        assert_eq!(space_of(&small, LENS), None);
    }

    #[test]
    fn tangents_map_onto_the_clip_square() {
        let ndc = to_ndc(&corners(), LENS, Space::Tangent);
        assert_eq!(ndc[0], [-1.0, -1.0]);
        assert_eq!(ndc[2], [1.0, 1.0]);
        // x = -0.6 is a third of the way across [-1.2, 0.9] (0.6 / 2.1), so -1 + 2 * 0.6 / 2.1.
        assert!((ndc[4][0] - (-1.0 + 2.0 * 0.6 / 2.1)).abs() < 1e-6);
        // y = -0.5 is 0.6 above -1.1 of 2.1: the same share.
        assert!((ndc[4][1] - (-1.0 + 2.0 * 0.6 / 2.1)).abs() < 1e-6);
    }

    #[test]
    fn a_mesh_with_a_bad_index_is_refused() {
        let area = HiddenArea {
            vertices: corners(),
            indices: vec![0, 1, 9],
        };
        assert!(EyeMask::new(&area, LENS).is_none());
        let ok = HiddenArea {
            vertices: corners(),
            indices: vec![0, 1, 4],
        };
        assert_eq!(EyeMask::new(&ok, LENS).unwrap().indices, vec![0, 1, 4]);
    }
}
