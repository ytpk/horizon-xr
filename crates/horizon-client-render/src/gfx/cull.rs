//! Occlusion culling of solid section draws, in two phases.
//!
//! Frustum and cave culling leave many sections that terrain in front hides
//! entirely (about half of the solid vertices drawn from the ground at render
//! distance 16, more at 32), and drawing fewer vertices is what helps an
//! integrated GPU. Phase 1 draws the sections that the last test read back
//! saw. Then, in the same pass, the box of every other section (and of a
//! share of phase 1's, to notice when they go out of sight) is drawn against
//! phase 1's depth with no writes but to the test's results. Phase 2, in a
//! second pass, draws the sections whose box was seen; the others' draws have
//! an instance count of 0.
//!
//! The picture stays the one without culling: the boxes around the mesh's
//! geometry (padded by a block) are drawn front faces
//! only, so a fragment is the box's nearest point on its pixel, and the
//! fragment shader runs only where the depth test passes (forced early), so a
//! section is left out only where all of its fragments would fail the depth
//! test against the final depth, which is only nearer. What the CPU knows of
//! the results only sorts sections between the phases: a stale result draws
//! a hidden section in phase 1, or tests a section that is in sight.
//!
//! Far terrain tiles are tested the same way, with their meshes' own
//! bounds for a box (grown by a block too, which keeps their faces off the
//! box's), and resident slots of their own (`TILE`).
//!
//! Every solid draw is an entry of one of the two lists, so the CPU records
//! as many draws as without culling: drivers that unroll an indirect
//! multi-draw on the CPU (Mesa's on Intel Gen9) pay per entry. Phase 2's
//! counts come from the GPU, which reads them from the draws themselves:
//! Vulkan's native multi-draw, DirectX 12's `ExecuteIndirect`, and Metal's,
//! which wgpu replays as one indirect draw per entry. Translucent draws are
//! not culled.
//!
//! Metal cannot force the depth test before a fragment shader that writes
//! storage (naga writes no `[[early_fragment_tests]]`), nor can DirectX 12
//! (`[earlydepthstencil]`, which wgpu doesn't offer there), so there the boxes
//! are drawn in a pass of their own between the phases, which reads phase
//! 1's depth as a texture and makes the same comparison in the shader: the
//! fragment's depth against the stored one, greater or equal passing. The
//! same fragments pass, so the same sections are seen.
use std::num::NonZeroU64;
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

const WORDS: u64 = 5 * 4;
/// A tested section's box, around its origin: its cell grown by a block.
pub const SECTION_BOX: [[f32; 3]; 2] = [[-1.0; 3], [17.0; 3]];
/// Mesh bounds already contain `BOUNDS_PAD`; grow them to a block of padding
/// in total, keeping the box's faces off the geometry. Without mesh bounds,
/// keep the section cell grown by a block, as before.
pub fn section_box(bounds: Option<[glam::Vec3; 2]>) -> [[f32; 3]; 2] {
    bounds.map_or(SECTION_BOX, |[low, high]| {
        let pad = glam::Vec3::splat(1.0 - crate::mesher::SectionMesh::BOUNDS_PAD);
        [(low - pad).to_array(), (high + pad).to_array()]
    })
}
/// The bit that marks a far tile's slot (`Renderer::tile_cull_slot`): tiles
/// count their slots apart from the sections' resident ones.
pub const TILE: u32 = 1 << 31;
/// Phase 1's sections are each tested every `RETEST` frames.
const RETEST: u32 = 8;
/// Culling runs while the last result read back hid at least this share of
/// the frame's solid indices: below it the test and the second pass cost
/// more than they save. Else one frame in `PROBE` tests every section.
const MIN_SHARE: f32 = 0.1;
const PROBE: u32 = 64;
/// The gate, for frame `stamp`, with `share` the newest result's hidden share:
/// whether the frame culls, and whether it probes (tests every section). GPU
/// culling's frames take it too.
pub(super) fn gate(share: f32, stamp: u32) -> (bool, bool) {
    let probe = share < MIN_SHARE;
    (!probe || stamp.is_multiple_of(PROBE), probe)
}
/// On Metal, sections in view from which a frame culls. Apple's GPUs store
/// and reload the whole frame at each of the passes the test adds, a fixed
/// cost that the sections it hides repay only at high render distances: on
/// an M4 Pro at 1080p and render distance 12 (1,000–1,600 draws) culling
/// added about 0.05 ms of GPU time while hiding 15–30% of the solid
/// indices; at 32 (over 4,096 draws) it saved 0.25–1.6 ms. Those draws were
/// 2.2 to 2.5 per section; a section's draw count now depends on the sides
/// it shows the eye (`mesher::Sides`), so the bar counts sections.
pub const TILED_MIN_SECTIONS: usize = 1600;
/// Boxes over more than this share of the screen count as seen untested.
const BIG: f32 = 0.25;
/// Results read back at once, some frames late so the CPU never waits.
const READBACKS: usize = 3;
const FREE: u8 = 0;
const PENDING: u8 = 1;
const MAPPED: u8 = 2;

/// One test, as `cull.wgsl` reads it: visible index and phase 2 draws
/// (`place`), then its box's corners around the origin.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Test {
    data: [u32; 4],
    low: [f32; 4],
    high: [f32; 4],
}
const TEST_BYTES: u64 = std::mem::size_of::<Test>() as u64;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    stamp: u32,
    big: f32,
    pad: [u32; 2],
    /// Each eye's view projection, which a stereo pass's boxes take (`recall`).
    eyes: [[[f32; 4]; 4]; 2],
}

/// Recall: a result older than this many frames no longer hides its section. A section
/// left out is tested every frame, and a result is back within `READBACKS` frames or so.
const FRESH: u32 = 5;
/// Whether a result of frame `tested` that did not see its section still hides it in frame
/// `now` (recall): only a recent one does. A section that left the view and comes back is
/// drawn until a new test says otherwise, and so is one never tested (`tested` 0).
pub(super) fn still_hidden(seen: bool, tested: u32, now: u32) -> bool {
    !seen && tested != 0 && now.wrapping_sub(tested) <= FRESH
}

/// One frame's results on their way back.
struct Readback {
    buffer: wgpu::Buffer,
    capacity: u64,
    state: Arc<AtomicU8>,
    stamp: u32,
    /// The resident slot of each test, and its section's solid indices.
    slots: Vec<u32>,
    indices: Vec<u32>,
    /// The frame's solid indices.
    total: u64,
}

/// The bind group and the buffers from outside it was made with.
struct Group {
    origins: wgpu::Buffer,
    globals: wgpu::Buffer,
    group: wgpu::BindGroup,
}

/// Where the boxes' depth test runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepthTest {
    /// Forced before the fragment shader, in phase 1's pass (Vulkan).
    Early,
    /// In the fragment shader, against phase 1's depth read as a texture,
    /// in a pass of its own (Metal).
    InShader,
}

/// The Vulkan entry point: the depth test forced early, which naga cannot
/// write for Metal (so it is not in `cull.wgsl`, which every backend takes).
const EARLY_TEST: &str = "
@fragment
@early_depth_test(force)
fn fs_box_early(in: Box) -> @location(0) vec4<f32> {
    see(in.test);
    return vec4(0.0);
}
";

/// The test's depth texture binding (`InShader`), for the view it was made with.
struct DepthGroup {
    view: wgpu::TextureView,
    group: wgpu::BindGroup,
}

pub struct Cull {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    group: Option<Group>,
    /// The boxes in a stereo (multiview) pass, each layer through its eye (`recall`).
    stereo_pipeline: Option<wgpu::RenderPipeline>,
    eyes: Option<[glam::Mat4; 2]>,
    /// Culling from results read back alone, in one pass (a headset's stereo pass, where
    /// there is no indirect multi-draw for phase 2): the sections the last result did not
    /// see are left out and tested every frame, the others drawn and tested in turn. A
    /// section that comes into sight is drawn when its result is back, a few frames late.
    /// Not on a Quest yet: its Adreno draws nothing at all through a fragment shader that
    /// writes a storage buffer, so no box is ever seen there (`multiview::occlusion_recall`).
    pub recall: bool,
    /// Per resident slot: the frame of the last result read back for it (0: none).
    tested: Vec<u32>,
    depth_test: DepthTest,
    depth_layout: Option<wgpu::BindGroupLayout>,
    depth_group: Option<DepthGroup>,
    params: wgpu::Buffer,
    /// Phase 2's draws: the solid draws after phase 1's in the frame's list.
    pub phase2: wgpu::Buffer,
    phase2_capacity: u64,
    tests: wgpu::Buffer,
    tests_capacity: u64,
    /// This frame's tests: visible index, then its phase 2 draws in each
    /// solid pass (`place`): the first of the first pass's, both counts (the
    /// second's in the high half), the first of the second pass's; and its box.
    test_data: Vec<Test>,
    test_slots: Vec<u32>,
    test_indices: Vec<u32>,
    total: u64,
    /// The frame tests every section, as culling was off.
    probe: bool,
    results: wgpu::Buffer,
    readbacks: Vec<Readback>,
    next: usize,
    current: Option<usize>,
    /// Per resident slot: whether the last result read back saw it; and per
    /// far tile's slot (`TILE`).
    seen: Vec<bool>,
    seen_tiles: Vec<bool>,
    stamp: u32,
    /// Of the boxes the last test read back drew, how many it did not see
    /// and how many it drew, and the share of solid indices hidden.
    pub hidden: ([usize; 2], f32),
    /// Cull every frame, whatever the hidden share (A/B checks).
    pub always: bool,
    /// Frames with fewer sections in view than this do not cull.
    pub min_sections: usize,
}

fn buffer(
    device: &wgpu::Device,
    label: &str,
    size: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage,
        mapped_at_creation: false,
    })
}

const PHASE2_USAGE: wgpu::BufferUsages = wgpu::BufferUsages::INDIRECT
    .union(wgpu::BufferUsages::STORAGE)
    .union(wgpu::BufferUsages::COPY_DST);
const TESTS_USAGE: wgpu::BufferUsages =
    wgpu::BufferUsages::STORAGE.union(wgpu::BufferUsages::COPY_DST);
const RESULTS_USAGE: wgpu::BufferUsages =
    wgpu::BufferUsages::STORAGE.union(wgpu::BufferUsages::COPY_SRC);
const READBACK_USAGE: wgpu::BufferUsages =
    wgpu::BufferUsages::MAP_READ.union(wgpu::BufferUsages::COPY_DST);

impl Cull {
    /// The device feature culling needs, where the adapter has it.
    pub fn features(adapter: &wgpu::Adapter) -> wgpu::Features {
        adapter.features() & wgpu::Features::SHADER_EARLY_DEPTH_TEST
    }
    /// Where the device can run the test, if it can: Vulkan with early
    /// depth tests, or Metal and DirectX 12 with the test in the shader, all
    /// drawing an indirect multi-draw's entries with the instance counts the
    /// GPU wrote, with fragment shaders that write storage.
    pub fn supported(adapter: &wgpu::Adapter, device: &wgpu::Device) -> Option<DepthTest> {
        let depth_test = match adapter.get_info().backend {
            wgpu::Backend::Vulkan | wgpu::Backend::Gl
                if device
                    .features()
                    .contains(wgpu::Features::SHADER_EARLY_DEPTH_TEST) =>
            {
                DepthTest::Early
            }
            // DirectX 12 has no forced early depth test in wgpu either: the test runs in the
            // shader there too.
            wgpu::Backend::Metal | wgpu::Backend::Dx12 => DepthTest::InShader,
            _ => return None,
        };
        (device
            .features()
            .contains(wgpu::Features::INDIRECT_FIRST_INSTANCE)
            && adapter.get_downlevel_capabilities().flags.contains(
                wgpu::DownlevelFlags::VERTEX_STORAGE
                    | wgpu::DownlevelFlags::FRAGMENT_WRITABLE_STORAGE,
            ))
        .then_some(depth_test)
    }
    /// Its test runs in phase 1's pass, the depth test forced early (Vulkan):
    /// GPU culling's frames can run it too.
    pub fn early(&self) -> bool {
        self.depth_test == DepthTest::Early
    }
    /// Whether the device can cull from results read back (`recall`): the boxes' fragments
    /// write their results after a forced early depth test. No indirect draw is needed.
    pub fn recall_supported(adapter: &wgpu::Adapter, device: &wgpu::Device) -> bool {
        adapter.get_info().backend == wgpu::Backend::Vulkan
            && device
                .features()
                .contains(wgpu::Features::SHADER_EARLY_DEPTH_TEST | wgpu::Features::MULTIVIEW)
            && adapter.get_downlevel_capabilities().flags.contains(
                wgpu::DownlevelFlags::VERTEX_STORAGE
                    | wgpu::DownlevelFlags::FRAGMENT_WRITABLE_STORAGE,
            )
    }
    /// `color` is the world pass's colour format; `recall`: it culls a stereo pass from
    /// read back results.
    pub fn new(
        device: &wgpu::Device,
        color: wgpu::TextureFormat,
        depth_test: DepthTest,
        recall: bool,
    ) -> Self {
        let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty,
            count: None,
        };
        let storage = |read_only| wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let uniform = wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let both = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let vertex = wgpu::ShaderStages::VERTEX;
        let fragment = wgpu::ShaderStages::FRAGMENT;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Section culling"),
            entries: &[
                entry(0, both, storage(true)),
                entry(1, fragment, storage(false)),
                entry(2, fragment, storage(false)),
                entry(3, vertex, storage(true)),
                entry(4, vertex, uniform),
                entry(5, both, uniform),
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Section culling"),
            source: wgpu::ShaderSource::Wgsl(source(depth_test).into()),
        });
        let depth_layout = (depth_test == DepthTest::InShader).then(|| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Section culling depth"),
                entries: &[entry(
                    0,
                    fragment,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                )],
            })
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Section culling"),
            bind_group_layouts: &[Some(&layout), depth_layout.as_ref()],
            ..Default::default()
        });
        // In the shader (`InShader`): no colour target, and the depth
        // attached read only (a pass needs an attachment), its test left to
        // the shader.
        let color_target = [Some(wgpu::ColorTargetState {
            format: color,
            blend: None,
            write_mask: wgpu::ColorWrites::empty(),
        })];
        let (fragment_entry, targets, depth_compare) = match depth_test {
            DepthTest::Early => (
                "fs_box_early",
                &color_target[..],
                wgpu::CompareFunction::GreaterEqual,
            ),
            DepthTest::InShader => ("fs_box", &[][..], wgpu::CompareFunction::Always),
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Section culling"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_box"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            // The world pass's reversed depth test, without writes.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(depth_compare),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(fragment_entry),
                compilation_options: Default::default(),
                targets,
            }),
            multiview_mask: None,
            cache: crate::gfx::pipeline_cache::get(device),
        });
        let stereo_pipeline = recall.then(|| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Section culling (stereo)"),
                source: wgpu::ShaderSource::Wgsl(stereo_source().into()),
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Section culling (stereo)"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_box"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: Some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_box_early"),
                    compilation_options: Default::default(),
                    targets: &color_target,
                }),
                multiview_mask: super::multiview::MASK,
                cache: crate::gfx::pipeline_cache::get(device),
            })
        });
        Self {
            stereo_pipeline,
            eyes: None,
            recall,
            tested: Vec::new(),
            pipeline,
            layout,
            group: None,
            depth_test,
            depth_layout,
            depth_group: None,
            params: buffer(
                device,
                "Section culling parameters",
                std::mem::size_of::<Params>() as u64,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            ),
            phase2: buffer(device, "Section draws, phase 2", WORDS, PHASE2_USAGE),
            phase2_capacity: 1,
            tests: buffer(device, "Section tests", TEST_BYTES, TESTS_USAGE),
            tests_capacity: 1,
            test_data: Vec::new(),
            test_slots: Vec::new(),
            test_indices: Vec::new(),
            total: 0,
            probe: false,
            results: buffer(device, "Section test results", 4, RESULTS_USAGE),
            readbacks: (0..READBACKS)
                .map(|_| Readback {
                    buffer: buffer(device, "Section test readback", 4, READBACK_USAGE),
                    capacity: 1,
                    state: Arc::new(AtomicU8::new(FREE)),
                    stamp: 0,
                    slots: Vec::new(),
                    indices: Vec::new(),
                    total: 0,
                })
                .collect(),
            next: 0,
            current: None,
            seen: Vec::new(),
            seen_tiles: Vec::new(),
            stamp: 0,
            hidden: ([0; 2], 1.0),
            always: false,
            min_sections: 0,
        }
    }

    /// Start a frame: take in the results that came back, and tell whether
    /// the frame culls.
    pub fn begin(&mut self, device: &wgpu::Device) -> bool {
        let _ = device.poll(wgpu::PollType::Poll);
        for i in 0..READBACKS {
            let r = &mut self.readbacks[(self.next + i) % READBACKS];
            if r.state.load(Ordering::Acquire) != MAPPED {
                continue;
            }
            let mapped = r.buffer.slice(..r.slots.len() as u64 * 4);
            if let Ok(data) = mapped.get_mapped_range() {
                // The mapping isn't guaranteed to be 4-byte aligned (a Vega 11
                // on Windows panicked in cast_slice), so copy the words out.
                let results: Vec<u32> = bytemuck::pod_collect_to_vec(&data);
                let (mut hidden, mut indices) = (0, 0u64);
                for (&result, &n) in results.iter().zip(&r.indices) {
                    if result != r.stamp {
                        hidden += 1;
                        indices += u64::from(n);
                    }
                }
                let share = indices as f32 / r.total.max(1) as f32;
                self.hidden = ([hidden, results.len()], share);
                for &slot in r.slots.iter().filter(|_| self.recall) {
                    let slot = slot as usize;
                    if slot >= self.tested.len() {
                        self.tested.resize(slot + 1, 0);
                    }
                    self.tested[slot] = r.stamp;
                }
                for (&slot, &result) in r.slots.iter().zip(&results) {
                    let (seen, slot) = if slot & TILE != 0 {
                        (&mut self.seen_tiles, (slot & !TILE) as usize)
                    } else {
                        (&mut self.seen, slot as usize)
                    };
                    if slot >= seen.len() {
                        seen.resize(slot + 1, false);
                    }
                    seen[slot] = result == r.stamp;
                }
            }
            r.buffer.unmap();
            r.state.store(FREE, Ordering::Release);
        }
        self.stamp = self.stamp.wrapping_add(1).max(1);
        self.test_data.clear();
        self.test_slots.clear();
        self.test_indices.clear();
        self.total = 0;
        let (culls, probe) = match self.always {
            true => (true, false),
            false => gate(self.hidden.1, self.stamp),
        };
        self.probe = probe;
        culls
    }

    /// Whether phase 1 draws the section in resident `slot` (or the far tile
    /// in its `TILE` slot).
    pub fn first(&self, slot: u32) -> bool {
        if slot & TILE != 0 {
            return self
                .seen_tiles
                .get((slot & !TILE) as usize)
                .copied()
                .unwrap_or(false);
        }
        self.seen.get(slot as usize).copied().unwrap_or(false)
    }

    /// Recall: whether the frame leaves out the section in resident `slot`, which a recent
    /// result did not see.
    pub fn left_out(&self, slot: u32) -> bool {
        let slot = slot as usize;
        still_hidden(
            self.seen.get(slot).copied().unwrap_or(true),
            self.tested.get(slot).copied().unwrap_or(0),
            self.stamp,
        )
    }

    /// Recall: a section the frame leaves out (`left_out`), of about `indices` solid
    /// indices: tested every frame, so it is drawn again soon after it comes into sight.
    pub fn hidden_test(&mut self, index: usize, slot: u32, indices: u32, bounds: [[f32; 3]; 2]) {
        self.total += u64::from(indices);
        self.push(index, bounds, slot, indices);
    }

    /// Recall: a section the frame draws: tested in turn, and at once while no recent
    /// result tells whether it is in sight.
    pub fn recall_drawn(&mut self, index: usize, slot: u32, indices: u32, bounds: [[f32; 3]; 2]) {
        self.total += u64::from(indices);
        let known = self
            .tested
            .get(slot as usize)
            .is_some_and(|&t| t != 0 && self.stamp.wrapping_sub(t) <= RETEST + FRESH);
        if indices > 0
            && (self.probe || !known || slot.wrapping_add(self.stamp).is_multiple_of(RETEST))
        {
            self.push(index, bounds, slot, indices);
        }
    }

    /// The eyes' view projections for a stereo pass's boxes (`recall`), or none.
    pub fn set_eyes(&mut self, eyes: Option<[glam::Mat4; 2]>) {
        self.eyes = eyes;
    }

    /// A section phase 1 draws, `index` in the visible list, with `indices`
    /// solid indices and the box `bounds` around its origin: tested in turn.
    pub fn drawn(&mut self, index: usize, slot: u32, indices: u32, bounds: [[f32; 3]; 2]) {
        self.total += u64::from(indices);
        if indices > 0 && (self.probe || slot.wrapping_add(self.stamp).is_multiple_of(RETEST)) {
            self.push(index, bounds, slot, indices);
        }
    }

    /// A section left to phase 2, with `draws` draws there: its test, whose
    /// draws `place` gives once phase 2's list is in its final order.
    pub fn deferred(
        &mut self,
        index: usize,
        slot: u32,
        draws: usize,
        indices: u32,
        bounds: [[f32; 3]; 2],
    ) -> Option<u32> {
        self.total += u64::from(indices);
        (draws > 0).then(|| {
            self.push(index, bounds, slot, indices);
            self.test_data.len() as u32 - 1
        })
    }

    /// `test`'s draws in solid pass `pass` (0 or 1): `draws` from phase 2's
    /// `first`. Each pass draws its own part of the section's quads.
    pub fn place(&mut self, test: u32, pass: u32, first: u32, draws: u32) {
        debug_assert!(pass < 2 && draws < 1 << 16);
        let t = &mut self.test_data[test as usize].data;
        t[1 + 2 * pass as usize] = first;
        t[2] |= draws << (16 * pass);
    }

    fn push(&mut self, index: usize, bounds: [[f32; 3]; 2], slot: u32, indices: u32) {
        let [low, high] = bounds.map(|[x, y, z]| [x, y, z, 0.0]);
        self.test_data.push(Test {
            data: [index as u32, 0, 0, 0],
            low,
            high,
        });
        self.test_slots.push(slot);
        self.test_indices.push(indices);
    }

    /// Stage this frame's inputs: phase 2's draws, with instance counts of 0,
    /// and the tests, which read the frame's camera-relative `origins` and
    /// view projection (the start of `globals`).
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        uploads: &mut super::upload::Uploads,
        encoder: &mut wgpu::CommandEncoder,
        phase2: &[[u32; 5]],
        origins: &wgpu::Buffer,
        globals: &wgpu::Buffer,
    ) {
        let draws = phase2.len() as u64;
        if draws > self.phase2_capacity {
            self.phase2_capacity = draws.next_power_of_two();
            self.phase2 = buffer(
                device,
                "Section draws, phase 2",
                self.phase2_capacity * WORDS,
                PHASE2_USAGE,
            );
            self.group = None;
        }
        let tests = self.test_data.len() as u64;
        if tests > self.tests_capacity {
            self.tests_capacity = tests.next_power_of_two();
            self.tests = buffer(
                device,
                "Section tests",
                self.tests_capacity * TEST_BYTES,
                TESTS_USAGE,
            );
            self.results = buffer(
                device,
                "Section test results",
                self.tests_capacity * 4,
                RESULTS_USAGE,
            );
            self.group = None;
        }
        if draws > 0 {
            uploads.buffer(encoder, &self.phase2, 0, bytemuck::cast_slice(phase2));
        }
        if tests > 0 {
            uploads.buffer(
                encoder,
                &self.tests,
                0,
                bytemuck::cast_slice(&self.test_data),
            );
        }
        let params = Params {
            stamp: self.stamp,
            big: BIG,
            pad: [0; 2],
            eyes: self
                .eyes
                .unwrap_or([glam::Mat4::IDENTITY; 2])
                .map(|eye| eye.to_cols_array_2d()),
        };
        uploads.buffer(encoder, &self.params, 0, bytemuck::bytes_of(&params));
        if !self
            .group
            .as_ref()
            .is_some_and(|g| g.origins == *origins && g.globals == *globals)
        {
            self.group = Some(self.make_group(device, origins, globals));
        }
    }

    /// In the world pass after phase 1 (`Early`): draw the tested boxes
    /// against its depth. Leaves the pass's pipeline and group 0 changed.
    pub fn test(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(Group { group, .. }) = &self.group else {
            return;
        };
        if self.depth_test != DepthTest::Early || self.test_data.is_empty() {
            return;
        }
        let boxes = self.test_data.len() as u32;
        // A stereo pass's boxes go through each eye's own view, in one draw without instances.
        if let (Some(stereo), Some(_)) = (&self.stereo_pipeline, self.eyes) {
            pass.set_pipeline(stereo);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..36 * boxes, 0..1);
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..36, 0..boxes);
    }

    /// Before the world pass (`InShader`): the binding of the depth the
    /// world is drawn with this frame.
    pub fn bind_depth(&mut self, device: &wgpu::Device, depth: &wgpu::TextureView) {
        let Some(layout) = &self.depth_layout else {
            return;
        };
        if self.depth_group.as_ref().is_none_or(|g| g.view != *depth) {
            self.depth_group = Some(DepthGroup {
                view: depth.clone(),
                group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Section culling depth"),
                    layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(depth),
                    }],
                }),
            });
        }
    }

    /// Between the phases (`InShader`): draw the tested boxes in a pass of
    /// their own that reads phase 1's depth, bound by `bind_depth`.
    pub fn test_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        timing: Option<&mut super::pack_timing::PassTiming>,
    ) {
        let (Some(Group { group, .. }), Some(DepthGroup { view, group: depth })) =
            (&self.group, &self.depth_group)
        else {
            return;
        };
        if self.test_data.is_empty() {
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Section culling"),
            timestamp_writes: timing.and_then(|t| t.writes("Occlusion test")),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view,
                depth_ops: None,
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.set_bind_group(1, depth, &[]);
        pass.draw(0..36, 0..self.test_data.len() as u32);
    }

    /// After the world passes: copy the results for reading back, when a
    /// readback buffer is free.
    pub fn end(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder) {
        let tests = self.test_data.len() as u64;
        let r = &mut self.readbacks[self.next];
        if tests == 0 || r.state.load(Ordering::Acquire) != FREE {
            return;
        }
        if tests > r.capacity {
            r.capacity = tests.next_power_of_two();
            r.buffer = buffer(
                device,
                "Section test readback",
                r.capacity * 4,
                READBACK_USAGE,
            );
        }
        encoder.copy_buffer_to_buffer(&self.results, 0, &r.buffer, 0, tests * 4);
        r.stamp = self.stamp;
        r.slots.clear();
        r.slots.extend_from_slice(&self.test_slots);
        r.indices.clear();
        r.indices.extend_from_slice(&self.test_indices);
        r.total = self.total;
        self.current = Some(self.next);
    }

    /// The frame's results to read back, started by calling it right after the frame's
    /// submission (on the submit thread, when the frame goes there).
    pub fn submitted(&mut self) -> Option<impl FnOnce() + Send + 'static> {
        let i = self.current.take()?;
        let r = &self.readbacks[i];
        let state = r.state.clone();
        state.store(PENDING, Ordering::Release);
        let (buffer, bytes) = (r.buffer.clone(), r.slots.len() as u64 * 4);
        self.next = (i + 1) % READBACKS;
        Some(move || {
            buffer
                .slice(..bytes)
                .map_async(wgpu::MapMode::Read, move |result| {
                    state.store(
                        if result.is_ok() { MAPPED } else { FREE },
                        Ordering::Release,
                    );
                });
        })
    }

    fn make_group(
        &self,
        device: &wgpu::Device,
        origins: &wgpu::Buffer,
        globals: &wgpu::Buffer,
    ) -> Group {
        fn whole(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry {
                binding,
                resource: buffer.as_entire_binding(),
            }
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Section culling"),
            layout: &self.layout,
            entries: &[
                whole(0, &self.tests),
                whole(1, &self.results),
                whole(2, &self.phase2),
                whole(3, origins),
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: globals,
                        offset: 0,
                        size: NonZeroU64::new(64),
                    }),
                },
                whole(5, &self.params),
            ],
        });
        Group {
            origins: origins.clone(),
            globals: globals.clone(),
            group,
        }
    }
}

/// The test's shader for `depth_test`.
fn source(depth_test: DepthTest) -> String {
    let common = include_str!("cull.wgsl");
    match depth_test {
        DepthTest::Early => format!("{common}{EARLY_TEST}"),
        DepthTest::InShader => common.to_owned(),
    }
}

/// The early test's shader for a stereo (multiview) pass: each layer's boxes through its
/// eye's view projection, from the parameters (a headset on Vulkan alone draws it).
fn stereo_source() -> String {
    let source = source(DepthTest::Early);
    let replaced = |source: String, from: &str, to: &str| {
        assert!(source.contains(from), "cull.wgsl lost `{from}`");
        source.replace(from, to)
    };
    let source = replaced(
        source,
        "    big: f32,\n}",
        "    big: f32,\n    eyes: array<mat4x4<f32>, 2>,\n}",
    );
    // No instances: one draw of every box's 36 vertices. Adreno's multiview draws gave the
    // boxes nothing to cover when each was an instance (no fragment ran, on any of them).
    let source = replaced(
        source,
        "fn vs_box(@builtin(vertex_index) v: u32, @builtin(instance_index) t: u32) -> Box {",
        "fn vs_box(@builtin(view_index) view: u32, @builtin(vertex_index) vertex: u32) -> Box {\n    let t = vertex / 36u;\n    let v = vertex % 36u;",
    );
    let source = replaced(
        source,
        "    let test = tests[t];\n",
        "    let test = tests[t];\n    var eye_view_proj = params.eyes[0];\n    if view == 1u { eye_view_proj = params.eyes[1]; }\n",
    );
    replaced(source, "globals.view_proj", "eye_view_proj")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stereo_shader_validates_and_takes_the_view() {
        let source = stereo_source();
        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
        let vertex = &module.entry_points[0];
        assert_eq!(vertex.name, "vs_box");
        assert!(
            vertex
                .function
                .arguments
                .iter()
                .any(|a| { a.binding == Some(naga::Binding::BuiltIn(naga::BuiltIn::ViewIndex)) })
        );
    }

    /// A real stereo pass: a box in front of both eyes is seen, one behind them is not, and
    /// one only the right eye's lens reaches is seen too.
    #[test]
    #[ignore = "requires a Vulkan adapter with MULTIVIEW; also runs on Mesa lavapipe"]
    fn a_stereo_pass_sees_the_boxes_either_eye_shows() {
        use wgpu::util::DeviceExt;
        let instance = super::super::test_gpu::instance();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let features = wgpu::Features::MULTIVIEW | wgpu::Features::SHADER_EARLY_DEPTH_TEST;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: features,
            required_limits: wgpu::Limits {
                max_multiview_view_count: 2,
                ..Default::default()
            },
            ..Default::default()
        }))
        .unwrap();
        assert!(Cull::recall_supported(&adapter, &device));
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut cull = Cull::new(&device, format, DepthTest::Early, true);
        cull.always = true;
        assert!(cull.begin(&device));
        // Ahead of both eyes, behind them, and far to the right: past the left eye's lens.
        let origins: [[f32; 4]; 3] = [
            [-8.0, -8.0, -60.0, 0.0],
            [-8.0, -8.0, 40.0, 0.0],
            [52.0, -8.0, -60.0, 0.0],
        ];
        for (i, _) in origins.iter().enumerate() {
            cull.recall_drawn(i, i as u32, 36, SECTION_BOX);
        }
        let lens = |tangents| super::super::headset::eye_projection(tangents, 0.05, 1000.0);
        cull.set_eyes(Some([
            lens([-1.0, 0.5, 1.0, -1.0]),
            lens([-0.5, 1.0, 1.0, -1.0]),
        ]));
        let buffer = |contents: &[u8], usage| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents,
                usage,
            })
        };
        let origins = buffer(bytemuck::cast_slice(&origins), wgpu::BufferUsages::STORAGE);
        let globals = buffer(&[0; 64], wgpu::BufferUsages::UNIFORM);
        let mut uploads = super::super::upload::Uploads::new(&device);
        let mut encoder = device.create_command_encoder(&Default::default());
        cull.upload(&device, &mut uploads, &mut encoder, &[], &origins, &globals);
        let target = super::super::multiview::Targets::new(&device, [64, 64], format);
        let view = target.color.create_view(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                multiview_mask: super::super::multiview::MASK,
                ..Default::default()
            });
            cull.test(&mut pass);
        }
        cull.end(&device, &mut encoder);
        uploads.finish(&encoder);
        queue.submit([encoder.finish()]);
        cull.submitted().unwrap()();
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(30)),
            })
            .unwrap();
        cull.begin(&device);
        assert_eq!(
            [cull.left_out(0), cull.left_out(1), cull.left_out(2)],
            [false, true, false]
        );
    }

    /// Recall leaves a section out only on a recent result that did not see it.
    #[test]
    fn only_a_recent_unseen_result_hides_a_section() {
        // Never tested, or seen: drawn.
        assert!(!still_hidden(false, 0, 40));
        assert!(!still_hidden(true, 38, 40));
        // Not seen two frames ago: left out.
        assert!(still_hidden(false, 38, 40));
        // Not seen long ago (it left the view since): drawn until tested again.
        assert!(!still_hidden(false, 40 - FRESH - 1, 40));
    }

    /// Culling while the newest result hid enough; else one frame in `PROBE`
    /// tests every section. GPU culling's frames take the same gate.
    #[test]
    fn the_gate_culls_while_enough_is_hidden_and_probes_else() {
        assert_eq!(gate(MIN_SHARE, 3), (true, false));
        assert_eq!(gate(MIN_SHARE / 2.0, 3), (false, true));
        assert_eq!(gate(MIN_SHARE / 2.0, PROBE), (true, true));
    }

    #[test]
    fn shaders_validate() {
        for (depth_test, capabilities) in [
            (DepthTest::Early, naga::valid::Capabilities::all()),
            // What wgpu's Metal backend allows: no early depth test.
            (
                DepthTest::InShader,
                naga::valid::Capabilities::all() - naga::valid::Capabilities::EARLY_DEPTH_TEST,
            ),
        ] {
            let module = naga::front::wgsl::parse_str(&source(depth_test)).unwrap();
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), capabilities)
                .validate(&module)
                .unwrap_or_else(|e| panic!("{depth_test:?}: {e:?}"));
        }
    }
}
