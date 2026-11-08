use std::cell::Cell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use scene_types::{
    normal_matrix, prepare_mesh, AddressMode, AlphaMode, ColorSpace, FilterMode, InstanceDesc,
    SamplerDesc, SceneData, TextureRef, ViewParams,
};
use wgpu::util::DeviceExt;

use crate::capture::PendingCapture;
use crate::{CapturedImage, GpuContext, RendererError, ShaderSet};

/// Linear HDR-capable scene colour format used between geometry and compose.
pub const SCENE_COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

static NEXT_RENDERER_ID: AtomicU32 = AtomicU32::new(1);

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuVertex {
    position: [f32; 3],
    uv: [f32; 2],
    normal: [f32; 3],
    tangent: [f32; 4],
    color: [f32; 4],
}

impl GpuVertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
        0 => Float32x3, 1 => Float32x2, 2 => Float32x3, 3 => Float32x4, 4 => Float32x4
    ];
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuInstance {
    model: [[f32; 4]; 4],
    normal: [[f32; 3]; 3],
    det_sign: f32,
}

impl GpuInstance {
    const ATTRIBUTES: [wgpu::VertexAttribute; 8] = wgpu::vertex_attr_array![
        5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Float32x4,
        9 => Float32x3, 10 => Float32x3, 11 => Float32x3, 12 => Float32
    ];

    fn from_desc(desc: &InstanceDesc) -> Self {
        // Validation has already rejected singular transforms.
        let (normal, det_sign) = normal_matrix(&desc.transform).unwrap_or(([[0.0; 3]; 3], 1.0));
        Self {
            model: desc.transform,
            normal,
            det_sign,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FrameUniform {
    view_pos: [f32; 4],
    view_proj: [[f32; 4]; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LightUniform {
    direction: [f32; 3],
    intensity: f32,
    colour: [f32; 3],
    ambient: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MaterialUniform {
    base_color_factor: [f32; 4],
    flags: [f32; 4],
}

/// Generational handle to an uploaded scene. Handles from another renderer
/// or a released scene are rejected with [`RendererError::InvalidHandle`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SceneId {
    renderer: u32,
    index: u32,
    generation: u32,
}

/// Resource counts for a scene, for diagnostics and tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SceneStats {
    pub meshes: usize,
    pub images: usize,
    pub materials: usize,
    pub instances: usize,
    /// Estimated bytes of vertex, index, instance and texture memory.
    pub gpu_bytes: u64,
}

/// Outcome of a successful upload.
#[derive(Debug, Clone, Default)]
pub struct UploadReport {
    /// Features present in the scene that the active profile does not display.
    pub diagnostics: Vec<String>,
    pub stats: SceneStats,
}

#[derive(Debug, Clone)]
pub struct RendererDesc {
    /// Format of target views passed to [`Renderer::encode`].
    pub output_format: wgpu::TextureFormat,
    /// Fail uploads using features the profile cannot display instead of
    /// rendering them approximately with a diagnostic.
    pub strict: bool,
}

impl RendererDesc {
    pub fn new(output_format: wgpu::TextureFormat) -> Self {
        Self {
            output_format,
            strict: false,
        }
    }
}

/// Size-dependent targets for one view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewDesc {
    pub width: u32,
    pub height: u32,
    /// Internal resolution multiplier (supersampling when > 1).
    pub render_scale: f32,
}

impl ViewDesc {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            render_scale: 1.0,
        }
    }
}

struct ViewTargets {
    color: wgpu::Texture,
    color_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    compose_bind_group: wgpu::BindGroup,
}

/// Per-view targets and uniforms. Views share scene resources but not
/// size-dependent targets. A zero-sized view is suspended.
pub struct RenderView {
    desc: ViewDesc,
    targets: Option<ViewTargets>,
    frame_buffer: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    light_buffer: wgpu::Buffer,
    light_bind_group: wgpu::BindGroup,
}

impl RenderView {
    pub fn desc(&self) -> ViewDesc {
        self.desc
    }

    pub fn is_suspended(&self) -> bool {
        self.targets.is_none()
    }

    pub fn aspect(&self) -> f32 {
        self.desc.width.max(1) as f32 / self.desc.height.max(1) as f32
    }
}

/// Identifies an encoded frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameReceipt {
    pub frame: u64,
    pub scene: Option<SceneId>,
}

/// What [`Renderer::encode_capture`] copies.
pub enum CaptureSource<'a> {
    /// The view's linear scene-colour target (internal resolution).
    SceneColor(&'a RenderView),
    /// Any supported texture with `COPY_SRC`, such as an offscreen output.
    Texture(&'a wgpu::Texture),
}

struct GpuMesh {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
}

struct GpuMaterial {
    bind_group: wgpu::BindGroup,
    double_sided: bool,
}

struct GpuScene {
    meshes: Vec<GpuMesh>,
    materials: Vec<GpuMaterial>,
    instance_buffer: Option<wgpu::Buffer>,
    instances: Vec<InstanceDesc>,
    stats: SceneStats,
    // Keep textures alive alongside the bind groups that reference them.
    _textures: Vec<wgpu::Texture>,
}

struct Slot {
    generation: u32,
    scene: Option<GpuScene>,
}

struct Fallbacks {
    white: wgpu::TextureView,
    flat_normal: wgpu::TextureView,
    orm: wgpu::TextureView,
}

pub struct Renderer {
    ctx: GpuContext,
    id: u32,
    desc: RendererDesc,
    material_layout: wgpu::BindGroupLayout,
    frame_layout: wgpu::BindGroupLayout,
    light_layout: wgpu::BindGroupLayout,
    compose_layout: wgpu::BindGroupLayout,
    geometry_double_sided: wgpu::RenderPipeline,
    geometry_culled: wgpu::RenderPipeline,
    compose: wgpu::RenderPipeline,
    compose_uniform: wgpu::Buffer,
    scene_sampler: wgpu::Sampler,
    samplers: HashMap<SamplerDesc, wgpu::Sampler>,
    fallbacks: Fallbacks,
    slots: Vec<Slot>,
    frame: Cell<u64>,
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn wgpu_address(mode: AddressMode) -> wgpu::AddressMode {
    match mode {
        AddressMode::ClampToEdge => wgpu::AddressMode::ClampToEdge,
        AddressMode::Repeat => wgpu::AddressMode::Repeat,
        AddressMode::MirrorRepeat => wgpu::AddressMode::MirrorRepeat,
    }
}

fn wgpu_filter(mode: FilterMode) -> wgpu::FilterMode {
    match mode {
        FilterMode::Nearest => wgpu::FilterMode::Nearest,
        FilterMode::Linear => wgpu::FilterMode::Linear,
    }
}

impl Renderer {
    /// Compile the shader set and create pipelines for `desc.output_format`.
    pub fn new(
        ctx: &GpuContext,
        shaders: &ShaderSet,
        desc: RendererDesc,
    ) -> Result<Self, RendererError> {
        let device = &ctx.device;
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material layout"),
            entries: &[
                texture_entry(0),
                sampler_entry(1),
                texture_entry(2),
                sampler_entry(3),
                texture_entry(4),
                sampler_entry(5),
                uniform_entry(6, wgpu::ShaderStages::FRAGMENT),
            ],
        });
        let vf = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame layout"),
            entries: &[uniform_entry(0, vf)],
        });
        let light_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("light layout"),
            entries: &[uniform_entry(0, vf)],
        });
        let compose_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("compose layout"),
            entries: &[
                texture_entry(0),
                sampler_entry(1),
                uniform_entry(2, wgpu::ShaderStages::FRAGMENT),
            ],
        });

        let geometry_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("geometry shader"),
            source: wgpu::ShaderSource::Wgsl(shaders.source("geometry").unwrap_or_default().into()),
        });
        let compose_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("compose shader"),
            source: wgpu::ShaderSource::Wgsl(shaders.source("compose").unwrap_or_default().into()),
        });

        let geometry_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("geometry pipeline layout"),
            bind_group_layouts: &[
                Some(&material_layout),
                Some(&frame_layout),
                Some(&light_layout),
            ],
            immediate_size: 0,
        });
        let geometry = |cull_mode| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("geometry pipeline"),
                layout: Some(&geometry_layout),
                vertex: wgpu::VertexState {
                    module: &geometry_module,
                    entry_point: Some("vertex_main"),
                    buffers: &[
                        wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<GpuVertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &GpuVertex::ATTRIBUTES,
                        },
                        wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<GpuInstance>() as u64,
                            step_mode: wgpu::VertexStepMode::Instance,
                            attributes: &GpuInstance::ATTRIBUTES,
                        },
                    ],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &geometry_module,
                    entry_point: Some("fragment_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: SCENE_COLOR_FORMAT,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                cache: None,
                multiview_mask: None,
            })
        };
        let geometry_double_sided = geometry(None);
        let geometry_culled = geometry(Some(wgpu::Face::Back));

        let compose_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("compose pipeline layout"),
                bind_group_layouts: &[Some(&compose_layout)],
                immediate_size: 0,
            });
        let compose = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("compose pipeline"),
            layout: Some(&compose_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &compose_module,
                entry_point: Some("vertex_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &compose_module,
                entry_point: Some("fragment_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: desc.output_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            cache: None,
            multiview_mask: None,
        });

        let manual_gamma = if desc.output_format.is_srgb() {
            0.0f32
        } else {
            1.0
        };
        let compose_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("compose uniform"),
            contents: bytemuck::cast_slice(&[manual_gamma, 0.0, 0.0, 0.0]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let scene_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("scene colour sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let solid = |label: &str, rgba: [u8; 4], format: wgpu::TextureFormat| {
            Self::create_texture(ctx, label, 1, 1, format, &rgba).create_view(&Default::default())
        };
        let fallbacks = Fallbacks {
            white: solid(
                "fallback base colour",
                [255; 4],
                wgpu::TextureFormat::Rgba8UnormSrgb,
            ),
            flat_normal: solid(
                "fallback normal",
                [128, 128, 255, 255],
                wgpu::TextureFormat::Rgba8Unorm,
            ),
            orm: solid(
                "fallback orm",
                [255, 200, 0, 255],
                wgpu::TextureFormat::Rgba8Unorm,
            ),
        };

        Ok(Self {
            ctx: ctx.clone(),
            id: NEXT_RENDERER_ID.fetch_add(1, Ordering::Relaxed),
            desc,
            material_layout,
            frame_layout,
            light_layout,
            compose_layout,
            geometry_double_sided,
            geometry_culled,
            compose,
            compose_uniform,
            scene_sampler,
            samplers: HashMap::new(),
            fallbacks,
            slots: Vec::new(),
            frame: Cell::new(0),
        })
    }

    pub fn context(&self) -> &GpuContext {
        &self.ctx
    }

    pub fn output_format(&self) -> wgpu::TextureFormat {
        self.desc.output_format
    }

    fn create_texture(
        ctx: &GpuContext,
        label: &str,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        rgba: &[u8],
    ) -> wgpu::Texture {
        ctx.device.create_texture_with_data(
            &ctx.queue,
            &wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            rgba,
        )
    }

    fn sampler(&mut self, desc: SamplerDesc) -> wgpu::Sampler {
        let device = &self.ctx.device;
        self.samplers
            .entry(desc)
            .or_insert_with(|| {
                device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("material sampler"),
                    address_mode_u: wgpu_address(desc.address_u),
                    address_mode_v: wgpu_address(desc.address_v),
                    mag_filter: wgpu_filter(desc.mag_filter),
                    min_filter: wgpu_filter(desc.min_filter),
                    ..Default::default()
                })
            })
            .clone()
    }

    fn check_limits(&self, scene: &SceneData) -> Result<(), RendererError> {
        let limits = self.ctx.device.limits();
        for (i, image) in scene.images.iter().enumerate() {
            if image.width.max(image.height) > limits.max_texture_dimension_2d {
                return Err(RendererError::Limits(format!(
                    "image {} is {}x{}, max dimension {}",
                    i, image.width, image.height, limits.max_texture_dimension_2d
                )));
            }
        }
        for (i, mesh) in scene.meshes.iter().enumerate() {
            let bytes = mesh.positions.len() as u64 * std::mem::size_of::<GpuVertex>() as u64;
            if bytes > limits.max_buffer_size {
                return Err(RendererError::Limits(format!(
                    "mesh {} vertex buffer is {} bytes",
                    i, bytes
                )));
            }
        }
        Ok(())
    }

    fn diagnostics(&self, scene: &SceneData) -> Result<Vec<String>, RendererError> {
        let mut diagnostics = Vec::new();
        for (i, material) in scene.materials.iter().enumerate() {
            if material.alpha_mode != AlphaMode::Opaque {
                diagnostics.push(format!(
                    "material {} ('{}') alpha mode {:?} is rendered opaque by profile 'nif-pbr-compat'",
                    i, material.name, material.alpha_mode
                ));
            }
        }
        if self.desc.strict && !diagnostics.is_empty() {
            return Err(RendererError::Unsupported(diagnostics.join("; ")));
        }
        Ok(diagnostics)
    }

    /// Validate and upload a scene. Nothing becomes visible to [`Self::encode`]
    /// unless every step succeeds; existing scenes are untouched on failure.
    pub fn upload_scene(
        &mut self,
        scene: &SceneData,
    ) -> Result<(SceneId, UploadReport), RendererError> {
        scene.validate()?;
        self.check_limits(scene)?;
        let diagnostics = self.diagnostics(scene)?;
        let mut stats = SceneStats {
            meshes: scene.meshes.len(),
            images: scene.images.len(),
            materials: scene.materials.len(),
            instances: scene.instances.len(),
            gpu_bytes: 0,
        };

        let device = self.ctx.device.clone();
        let mut meshes = Vec::with_capacity(scene.meshes.len());
        for mesh in &scene.meshes {
            let prepared = prepare_mesh(mesh);
            let vertices: Vec<GpuVertex> = (0..prepared.positions.len())
                .map(|i| GpuVertex {
                    position: prepared.positions[i],
                    uv: prepared.uvs[i],
                    normal: prepared.normals[i],
                    tangent: prepared.tangents[i],
                    color: prepared.colors[i],
                })
                .collect();
            stats.gpu_bytes += (vertices.len() * std::mem::size_of::<GpuVertex>()
                + prepared.indices.len() * 4) as u64;
            meshes.push(GpuMesh {
                vertex_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(&format!("{} vertices", mesh.name)),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                index_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(&format!("{} indices", mesh.name)),
                    contents: bytemuck::cast_slice(&prepared.indices),
                    usage: wgpu::BufferUsages::INDEX,
                }),
                index_count: prepared.indices.len() as u32,
            });
        }

        let mut textures = Vec::with_capacity(scene.images.len());
        let mut views = Vec::with_capacity(scene.images.len());
        for image in &scene.images {
            let format = match image.color_space {
                ColorSpace::Srgb => wgpu::TextureFormat::Rgba8UnormSrgb,
                ColorSpace::Linear => wgpu::TextureFormat::Rgba8Unorm,
            };
            let texture = Self::create_texture(
                &self.ctx,
                &image.label,
                image.width,
                image.height,
                format,
                &image.pixels,
            );
            stats.gpu_bytes += image.pixels.len() as u64;
            views.push(texture.create_view(&Default::default()));
            textures.push(texture);
        }

        let (white, flat, default_orm) = (
            self.fallbacks.white.clone(),
            self.fallbacks.flat_normal.clone(),
            self.fallbacks.orm.clone(),
        );
        let mut materials = Vec::with_capacity(scene.materials.len());
        for material in &scene.materials {
            let mut bind = |t: &Option<TextureRef>, fallback: &wgpu::TextureView| {
                let sampler = self.sampler(t.map(|t| t.sampler).unwrap_or_default());
                let view = t.map_or_else(|| fallback.clone(), |t| views[t.image].clone());
                (view, sampler)
            };
            let base = bind(&material.base_color_texture, &white);
            let normal = bind(&material.normal_texture, &flat);
            let orm = bind(&material.orm_texture, &default_orm);
            let uniform = MaterialUniform {
                base_color_factor: material.base_color_factor,
                flags: [
                    if material.use_vertex_colors { 1.0 } else { 0.0 },
                    0.0,
                    0.0,
                    0.0,
                ],
            };
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("material uniform"),
                contents: bytemuck::bytes_of(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&material.name),
                layout: &self.material_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&base.0),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&base.1),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&normal.0),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&normal.1),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(&orm.0),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::Sampler(&orm.1),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: buffer.as_entire_binding(),
                    },
                ],
            });
            materials.push(GpuMaterial {
                bind_group,
                double_sided: material.double_sided,
            });
        }

        let instance_buffer = self.create_instance_buffer(&scene.instances);
        stats.gpu_bytes += (scene.instances.len() * std::mem::size_of::<GpuInstance>()) as u64;

        let gpu = GpuScene {
            meshes,
            materials,
            instance_buffer,
            instances: scene.instances.clone(),
            stats,
            _textures: textures,
        };
        let index = match self.slots.iter().position(|s| s.scene.is_none()) {
            Some(i) => i,
            None => {
                self.slots.push(Slot {
                    generation: 0,
                    scene: None,
                });
                self.slots.len() - 1
            }
        };
        let slot = &mut self.slots[index];
        slot.generation = slot.generation.wrapping_add(1);
        slot.scene = Some(gpu);
        let id = SceneId {
            renderer: self.id,
            index: index as u32,
            generation: slot.generation,
        };
        for d in &diagnostics {
            log::warn!("{}", d);
        }
        Ok((id, UploadReport { diagnostics, stats }))
    }

    fn create_instance_buffer(&self, instances: &[InstanceDesc]) -> Option<wgpu::Buffer> {
        if instances.is_empty() {
            return None;
        }
        let raw: Vec<GpuInstance> = instances.iter().map(GpuInstance::from_desc).collect();
        Some(
            self.ctx
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("instances"),
                    contents: bytemuck::cast_slice(&raw),
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                }),
        )
    }

    fn scene(&self, id: SceneId) -> Result<&GpuScene, RendererError> {
        if id.renderer != self.id {
            return Err(RendererError::InvalidHandle(id));
        }
        self.slots
            .get(id.index as usize)
            .filter(|s| s.generation == id.generation)
            .and_then(|s| s.scene.as_ref())
            .ok_or(RendererError::InvalidHandle(id))
    }

    fn scene_mut(&mut self, id: SceneId) -> Result<&mut GpuScene, RendererError> {
        self.scene(id)?;
        Ok(self.slots[id.index as usize].scene.as_mut().unwrap())
    }

    /// Replace instance transforms/visibility. The count and mesh/material
    /// references must remain valid for the uploaded scene.
    pub fn update_instances(
        &mut self,
        id: SceneId,
        instances: &[InstanceDesc],
    ) -> Result<(), RendererError> {
        let scene = self.scene(id)?;
        for (i, instance) in instances.iter().enumerate() {
            if instance.mesh >= scene.meshes.len() || instance.material >= scene.materials.len() {
                return Err(scene_types::SceneError::MissingReference {
                    what: format!("instance {}", i),
                    index: instance.mesh.max(instance.material),
                    len: scene.meshes.len().min(scene.materials.len()),
                }
                .into());
            }
            if !instance.transform.iter().flatten().all(|v| v.is_finite()) {
                return Err(scene_types::SceneError::NonFinite {
                    what: format!("instance {} transform", i),
                }
                .into());
            }
            if normal_matrix(&instance.transform).is_none() {
                return Err(scene_types::SceneError::SingularTransform { instance: i }.into());
            }
        }
        let same_len = instances.len() == scene.instances.len();
        if same_len {
            if let Some(buffer) = &scene.instance_buffer {
                let raw: Vec<GpuInstance> = instances.iter().map(GpuInstance::from_desc).collect();
                self.ctx
                    .queue
                    .write_buffer(buffer, 0, bytemuck::cast_slice(&raw));
            }
        }
        let buffer = if same_len {
            None
        } else {
            Some(self.create_instance_buffer(instances))
        };
        let scene = self.scene_mut(id)?;
        if let Some(buffer) = buffer {
            scene.instance_buffer = buffer;
        }
        scene.instances = instances.to_vec();
        scene.stats.instances = instances.len();
        Ok(())
    }

    /// Release a scene's GPU resources. In-flight submissions keep them alive
    /// until complete. Returns false for unknown or already released handles.
    pub fn release_scene(&mut self, id: SceneId) -> bool {
        if self.scene(id).is_err() {
            return false;
        }
        self.slots[id.index as usize].scene = None;
        true
    }

    pub fn scene_stats(&self, id: SceneId) -> Result<SceneStats, RendererError> {
        Ok(self.scene(id)?.stats)
    }

    pub fn live_scene_count(&self) -> usize {
        self.slots.iter().filter(|s| s.scene.is_some()).count()
    }

    pub fn create_view(&self, desc: ViewDesc) -> Result<RenderView, RendererError> {
        let device = &self.ctx.device;
        let frame_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame uniform"),
            size: std::mem::size_of::<FrameUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let light_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("light uniform"),
            size: std::mem::size_of::<LightUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = |layout, buffer: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            })
        };
        let mut view = RenderView {
            desc,
            targets: None,
            frame_bind_group: bind(&self.frame_layout, &frame_buffer),
            light_bind_group: bind(&self.light_layout, &light_buffer),
            frame_buffer,
            light_buffer,
        };
        self.resize_view(&mut view, desc)?;
        Ok(view)
    }

    /// Reallocate size-dependent targets. Zero-sized views are suspended.
    pub fn resize_view(&self, view: &mut RenderView, desc: ViewDesc) -> Result<(), RendererError> {
        if !(desc.render_scale.is_finite() && desc.render_scale > 0.0) {
            return Err(RendererError::Limits(format!(
                "render scale {}",
                desc.render_scale
            )));
        }
        let width = (desc.width as f32 * desc.render_scale) as u32;
        let height = (desc.height as f32 * desc.render_scale) as u32;
        let max = self.ctx.device.limits().max_texture_dimension_2d;
        if width > max || height > max {
            return Err(RendererError::Limits(format!(
                "render target {}x{} exceeds {}",
                width, height, max
            )));
        }
        view.desc = desc;
        if width == 0 || height == 0 {
            view.targets = None;
            return Ok(());
        }
        let device = &self.ctx.device;
        let target = |label, format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let color = target(
            "scene colour",
            SCENE_COLOR_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        );
        let depth = target(
            "scene depth",
            DEPTH_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let color_view = color.create_view(&Default::default());
        let compose_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("compose bind group"),
            layout: &self.compose_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&color_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.scene_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.compose_uniform.as_entire_binding(),
                },
            ],
        });
        view.targets = Some(ViewTargets {
            depth_view: depth.create_view(&Default::default()),
            color,
            color_view,
            compose_bind_group,
        });
        Ok(())
    }

    /// Encode one frame of `scene` (or an empty frame) into `target`, which
    /// must have the renderer's output format and the view's size.
    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &RenderView,
        scene: Option<SceneId>,
        params: &ViewParams,
        target: &wgpu::TextureView,
    ) -> Result<FrameReceipt, RendererError> {
        let targets = view.targets.as_ref().ok_or(RendererError::Suspended)?;
        let gpu_scene = scene.map(|id| self.scene(id)).transpose()?;

        let camera = &params.camera;
        let frame_uniform = FrameUniform {
            view_pos: [camera.eye[0], camera.eye[1], camera.eye[2], 1.0],
            view_proj: camera.view_projection(view.aspect()),
        };
        let light = &params.light;
        let light_uniform = LightUniform {
            direction: light.normalized_direction(),
            intensity: light.intensity,
            colour: light.colour,
            ambient: light.ambient,
        };
        let queue = &self.ctx.queue;
        queue.write_buffer(&view.frame_buffer, 0, bytemuck::bytes_of(&frame_uniform));
        queue.write_buffer(&view.light_buffer, 0, bytemuck::bytes_of(&light_uniform));

        let [r, g, b, a] = params.clear_color;
        let clear = wgpu::Color { r, g, b, a };
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("geometry pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(scene) = gpu_scene {
                if let Some(instance_buffer) = &scene.instance_buffer {
                    pass.set_bind_group(1, &view.frame_bind_group, &[]);
                    pass.set_bind_group(2, &view.light_bind_group, &[]);
                    pass.set_vertex_buffer(1, instance_buffer.slice(..));
                    for (i, instance) in scene.instances.iter().enumerate() {
                        if !instance.visible {
                            continue;
                        }
                        let mesh = &scene.meshes[instance.mesh];
                        let material = &scene.materials[instance.material];
                        pass.set_pipeline(if material.double_sided {
                            &self.geometry_double_sided
                        } else {
                            &self.geometry_culled
                        });
                        pass.set_bind_group(0, &material.bind_group, &[]);
                        pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                        pass.set_index_buffer(
                            mesh.index_buffer.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        let i = i as u32;
                        pass.draw_indexed(0..mesh.index_count, 0, i..i + 1);
                    }
                }
            }
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("compose pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.compose);
            pass.set_bind_group(0, &targets.compose_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        let frame = self.frame.get() + 1;
        self.frame.set(frame);
        Ok(FrameReceipt { frame, scene })
    }

    /// Encode a readback copy after a frame's passes, in the same encoder.
    pub fn encode_capture(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        receipt: &FrameReceipt,
        source: CaptureSource<'_>,
        label: &str,
    ) -> Result<PendingCapture, RendererError> {
        let texture = match source {
            CaptureSource::SceneColor(view) => {
                &view.targets.as_ref().ok_or(RendererError::Suspended)?.color
            }
            CaptureSource::Texture(texture) => texture,
        };
        PendingCapture::encode(&self.ctx.device, encoder, texture, label, receipt.frame)
    }

    /// Texture suitable as an offscreen [`Self::encode`] target and capture source.
    pub fn create_output_texture(&self, width: u32, height: u32) -> wgpu::Texture {
        self.ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen output"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.desc.output_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }

    /// Render one frame offscreen and read back the output.
    pub async fn render_to_image(
        &self,
        view: &RenderView,
        scene: Option<SceneId>,
        params: &ViewParams,
    ) -> Result<CapturedImage, RendererError> {
        let desc = view.desc();
        let output = self.create_output_texture(desc.width, desc.height);
        let output_view = output.create_view(&Default::default());
        let mut encoder = self.ctx.create_encoder("offscreen frame");
        let receipt = self.encode(&mut encoder, view, scene, params, &output_view)?;
        let capture = self.encode_capture(
            &mut encoder,
            &receipt,
            CaptureSource::Texture(&output),
            "output",
        )?;
        self.ctx.submit(encoder);
        let (sender, receiver) = futures::channel::oneshot::channel();
        capture.map(move |result| {
            let _ = sender.send(result);
        });
        #[cfg(not(target_arch = "wasm32"))]
        self.ctx
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| RendererError::Capture(e.to_string()))?;
        receiver
            .await
            .map_err(|_| RendererError::Capture("readback cancelled".into()))?
    }
}
