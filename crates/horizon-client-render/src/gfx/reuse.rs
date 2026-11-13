//! Pipelines kept between the renderers built on one device. The start builds
//! three (the boot screen's, the title artwork's, the world's) and a resource
//! reload or a graphics option builds another; each compiled every shader
//! module and pipeline again, though most come out identical (the boot and
//! title renderers' entirely, the world's entity pipelines). On a driver that
//! compiles slowly (a cold shader cache, llvmpipe, a weak CPU) that was seconds
//! of the main thread before the title showed.
//!
//! An object is kept under its descriptor's `Debug` text: wgpu prints the
//! labels, sources, constants, formats and states, and the handles a descriptor
//! names (shader modules, layouts) by their ids, so a descriptor that names a
//! kept handle matches only the same handle. Kept objects stay alive, so their
//! ids are never reused while they are keys.
use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

/// One map of kept objects with the generation each was last asked for in.
struct Kept<T> {
    map: Mutex<HashMap<String, (T, u32)>>,
}
impl<T> Default for Kept<T> {
    fn default() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }
}
impl<T: Clone> Kept<T> {
    /// The object kept under `key`, else `make`'s, kept. The lock is not
    /// held while `make` compiles.
    fn get(&self, key: String, generation: u32, make: impl FnOnce() -> T) -> T {
        if let Some((object, used)) = self
            .map
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_mut(&key)
        {
            *used = generation;
            return object.clone();
        }
        let object = make();
        self.map
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key, (object.clone(), generation));
        object
    }
    fn keep_only(&self, generation: u32) {
        self.map
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|_, (_, used)| *used == generation);
    }
    fn len(&self) -> usize {
        self.map
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

/// The shader modules, layouts and pipelines of the device's renderers.
#[derive(Default)]
pub struct Reuse {
    generation: std::sync::atomic::AtomicU32,
    stereo_shaders: Mutex<HashMap<wgpu::ShaderModule, wgpu::ShaderModule>>,
    stereo_pipelines: Mutex<HashMap<wgpu::RenderPipeline, wgpu::RenderPipeline>>,
    shaders: Kept<wgpu::ShaderModule>,
    bind_group_layouts: Kept<wgpu::BindGroupLayout>,
    pipeline_layouts: Kept<wgpu::PipelineLayout>,
    render: Kept<wgpu::RenderPipeline>,
    tried: Kept<Option<wgpu::RenderPipeline>>,
}

impl Reuse {
    /// A renderer starts building: what it asks for is kept, the rest is let
    /// go by [`Reuse::finish`].
    pub fn begin(&self) -> u32 {
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .wrapping_add(1)
    }
    /// The renderer of `generation` is built: objects only older renderers
    /// used are dropped here (the renderers still alive hold their own).
    pub fn finish(&self, generation: u32) {
        self.shaders.keep_only(generation);
        self.bind_group_layouts.keep_only(generation);
        self.pipeline_layouts.keep_only(generation);
        self.render.keep_only(generation);
        self.tried.keep_only(generation);
        let shaders: Vec<_> = self
            .shaders
            .map
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .map(|(v, _)| v.clone())
            .collect();
        self.stereo_shaders
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|k, _| shaders.contains(k));
        let pipelines: Vec<_> = self
            .render
            .map
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .map(|(v, _)| v.clone())
            .collect();
        self.stereo_pipelines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|k, v| pipelines.contains(k) || pipelines.contains(v));
    }
    /// How many objects are kept.
    pub fn len(&self) -> usize {
        self.shaders.len()
            + self.bind_group_layouts.len()
            + self.pipeline_layouts.len()
            + self.render.len()
            + self.tried.len()
    }
    #[cfg(test)]
    pub(super) fn stereo_for_test<'a>(&'a self, device: &'a wgpu::Device) -> On<'a> {
        let mut on = self.on(device);
        on.stereo = true;
        on
    }
    pub fn alternates<'a>(
        &self,
        pipelines: impl Iterator<Item = &'a wgpu::RenderPipeline>,
    ) -> Option<Vec<wgpu::RenderPipeline>> {
        let variants = self
            .stereo_pipelines
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        pipelines.map(|p| variants.get(p).cloned()).collect()
    }
    fn current(&self) -> u32 {
        self.generation.load(std::sync::atomic::Ordering::Relaxed)
    }
    /// `device`'s `create_*` calls through this cache.
    pub fn on<'a>(&'a self, device: &'a wgpu::Device) -> On<'a> {
        On {
            reuse: self,
            device,
            generation: self.current(),
            stereo: stereo_enabled(device),
        }
    }
}

/// A device whose shader modules, layouts and render pipelines come from a
/// [`Reuse`] when one with the same descriptor is kept.
pub struct On<'a> {
    reuse: &'a Reuse,
    device: &'a wgpu::Device,
    generation: u32,
    stereo: bool,
}
impl On<'_> {
    /// Whether pipelines made here get a stereo (multiview) variant too.
    pub fn stereo(&self) -> bool {
        self.stereo
    }
    pub fn create_shader_module(
        &self,
        descriptor: wgpu::ShaderModuleDescriptor<'_>,
    ) -> wgpu::ShaderModule {
        let key = format!("{descriptor:?}");
        let module = self.reuse.shaders.get(key, self.generation, || {
            self.device.create_shader_module(descriptor.clone())
        });
        if self.stereo {
            let mut variants = self
                .reuse
                .stereo_shaders
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !variants.contains_key(&module)
                && let wgpu::ShaderSource::Wgsl(source) = &descriptor.source
            {
                let source = super::multiview::shader(source);
                let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
                let variant = self
                    .device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: descriptor.label,
                        source: wgpu::ShaderSource::Wgsl(source.into()),
                    });
                if let Some(error) = pollster::block_on(scope.pop()) {
                    log::warn!("Quest multiview shader unavailable: {error}");
                } else {
                    variants.insert(module.clone(), variant);
                }
            }
        }
        module
    }
    pub fn create_bind_group_layout(
        &self,
        descriptor: &wgpu::BindGroupLayoutDescriptor<'_>,
    ) -> wgpu::BindGroupLayout {
        let key = format!("{descriptor:?}");
        self.reuse.bind_group_layouts.get(key, self.generation, || {
            self.device.create_bind_group_layout(descriptor)
        })
    }
    pub fn create_pipeline_layout(
        &self,
        descriptor: &wgpu::PipelineLayoutDescriptor<'_>,
    ) -> wgpu::PipelineLayout {
        let key = format!("{descriptor:?}");
        self.reuse.pipeline_layouts.get(key, self.generation, || {
            self.device.create_pipeline_layout(descriptor)
        })
    }
    /// The multiview variant of `pipeline` (made from `descriptor`), kept beside it for the
    /// headset's stereo swap. Every route that makes a render pipeline ends here, `try_`
    /// ones included: a pipeline without one keeps the whole world out of multiview.
    fn add_stereo(
        &self,
        descriptor: &wgpu::RenderPipelineDescriptor<'_>,
        pipeline: &wgpu::RenderPipeline,
    ) {
        if self.stereo && descriptor.multiview_mask.is_none() {
            let mut variants = self
                .reuse
                .stereo_pipelines
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !variants.contains_key(pipeline) {
                let shaders = self
                    .reuse
                    .stereo_shaders
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                let vertex = shaders.get(descriptor.vertex.module);
                let fragment = descriptor
                    .fragment
                    .as_ref()
                    .and_then(|f| shaders.get(f.module));
                if let Some(vertex) = vertex
                    && (descriptor.fragment.is_none() || fragment.is_some())
                {
                    let mut stereo = descriptor.clone();
                    stereo.vertex.module = vertex;
                    if let Some(f) = stereo.fragment.as_mut() {
                        f.module = fragment.unwrap();
                    }
                    stereo.multiview_mask = super::multiview::MASK;
                    let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
                    let variant = self.device.create_render_pipeline(&stereo);
                    if let Some(error) = pollster::block_on(scope.pop()) {
                        log::warn!(
                            "Quest multiview pipeline {:?} unavailable: {error}",
                            descriptor.label
                        );
                    } else {
                        variants.insert(pipeline.clone(), variant.clone());
                        variants.insert(variant, pipeline.clone());
                    }
                }
            }
        }
    }
    pub fn create_render_pipeline(
        &self,
        descriptor: &wgpu::RenderPipelineDescriptor<'_>,
    ) -> wgpu::RenderPipeline {
        let mut cached = descriptor.clone();
        cached.cache = cached
            .cache
            .or_else(|| super::pipeline_cache::get(self.device));
        let descriptor = &cached;
        let key = format!("{descriptor:?}");
        let pipeline = self.reuse.render.get(key, self.generation, || {
            self.device.create_render_pipeline(descriptor)
        });
        self.add_stereo(descriptor, &pipeline);
        pipeline
    }
    /// `create_render_pipeline` for a path only some GPUs take: None where the device refuses it,
    /// so the frame can keep the usual pipeline. A refusal is kept too: only the first attempt
    /// reports one, and a pipeline kept after it would be invalid.
    pub fn try_create_render_pipeline(
        &self,
        descriptor: &wgpu::RenderPipelineDescriptor<'_>,
    ) -> Option<wgpu::RenderPipeline> {
        let key = format!("{descriptor:?}");
        let pipeline = self.reuse.tried.get(key, self.generation, || {
            let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let internal = self.device.push_error_scope(wgpu::ErrorFilter::Internal);
            let pipeline = self.device.create_render_pipeline(descriptor);
            let internal = pollster::block_on(internal.pop());
            match internal.or(pollster::block_on(validation.pop())) {
                Some(error) => {
                    log::warn!("{:?} unavailable: {error}", descriptor.label);
                    None
                }
                None => Some(pipeline),
            }
        });
        if let Some(pipeline) = &pipeline {
            self.add_stereo(descriptor, pipeline);
        }
        pipeline
    }
}

fn stereo_enabled(device: &wgpu::Device) -> bool {
    device.features().contains(wgpu::Features::MULTIVIEW)
        && super::headset::installed()
        && horizon_client_input::headset::settings().bool("horizonMultiview")
}

#[cfg(test)]
mod tests {
    use super::{Kept, Reuse};

    /// A pipeline the device refuses comes back as None, without an uncaptured error and without
    /// trying again; a good one is kept.
    #[test]
    fn a_refused_pipeline_is_none_and_stays_none() {
        let instance = crate::gfx::test_gpu::instance();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let Ok((device, _)) = pollster::block_on(adapter.request_device(&Default::default()))
        else {
            eprintln!("no GPU device: skipped");
            return;
        };
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(
                "@vertex fn vs() -> @builtin(position) vec4<f32> { return vec4(0.0); }".into(),
            ),
        });
        let reuse = Reuse::default();
        reuse.begin();
        let kept = reuse.on(&device);
        let descriptor = |entry| wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        };
        assert!(kept.try_create_render_pipeline(&descriptor("vs")).is_some());
        assert!(kept.try_create_render_pipeline(&descriptor("no")).is_none());
        assert!(kept.try_create_render_pipeline(&descriptor("no")).is_none());
        assert_eq!(reuse.len(), 2);
    }

    /// A key asked for again gets the kept object without making one; what
    /// the latest generation did not ask for is let go when it finishes.
    #[test]
    fn kept_objects_are_shared_until_a_build_stops_asking() {
        let kept = Kept::<u32>::default();
        let made = std::cell::Cell::new(0);
        let make = |value| {
            made.set(made.get() + 1);
            value
        };
        assert_eq!(kept.get("a".into(), 1, || make(10)), 10);
        assert_eq!(kept.get("b".into(), 1, || make(20)), 20);
        kept.keep_only(1);
        assert_eq!(kept.len(), 2);
        // The next build asks for "a" only.
        assert_eq!(kept.get("a".into(), 2, || make(11)), 10);
        kept.keep_only(2);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept.get("b".into(), 3, || make(21)), 21);
        assert_eq!(made.get(), 3);
    }
}
