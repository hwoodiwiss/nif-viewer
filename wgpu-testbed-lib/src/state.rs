use std::collections::HashMap;
use std::iter::FromIterator;
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc;

use crate::camera::CameraController;
use crate::file_reader::FileReader;
use crate::instance::InstanceRaw;
use crate::pipeline::{self, create_render_pipeline};
use crate::uniform::Uniforms;
use crate::{instance::Instance, light::Light};
use cgmath::*;

use log::info;
use wgpu::util::DeviceExt;
use wgpu::{
    ExperimentalFeatures, InstanceDescriptor, PowerPreference, SamplerBindingType,
    TexelCopyBufferInfo, TexelCopyTextureInfo,
};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::{event::WindowEvent, window::Window};

use crate::camera::Camera;
use crate::model::{self, DrawLight, Material, Mesh, ModelLoader, QuadVertex};
use crate::model::{DrawModel, Model};
use crate::texture::{self, Texture};
use crate::vertex::Vertex;

const fn rgb_to_normalized(r: u8, g: u8, b: u8) -> wgpu::Color {
    wgpu::Color {
        r: r as f64 / 255f64,
        g: g as f64 / 255f64,
        b: b as f64 / 255f64,
        a: 1.0,
    }
}

const INSTANCES_PER_ROW: u32 = 100;
const INSTANCE_DISPLACEMENT: cgmath::Vector3<f32> = cgmath::Vector3::new(
    INSTANCES_PER_ROW as f32 * 0.5,
    0.0,
    INSTANCES_PER_ROW as f32 * 0.5,
);

const RENDER_SCALE: f32 = 2.0;

pub struct MappedTextureView {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub unpadded_bytes_per_row: u32,
    pub padded_bytes_per_row: u32,
    pub format: wgpu::TextureFormat,
    pub name: String,
}

pub struct State<'a> {
    surface: wgpu::Surface<'a>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: wgpu::SurfaceConfiguration,
    pub size: winit::dpi::PhysicalSize<u32>,
    bg_color: wgpu::Color,
    deferred_render_pipeline: wgpu::RenderPipeline,
    camera: Camera,
    uniforms: Uniforms,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    camera_controller: CameraController,
    instances: Vec<Instance>,
    instance_buffer: wgpu::Buffer,
    depth_texture: Texture,
    obj_model: Model,
    screen_quad: Mesh,
    render_material: Material,
    light: Light,
    light_buffer: wgpu::Buffer,
    light_bind_group: wgpu::BindGroup,
    light_render_pipeline: wgpu::RenderPipeline,
    output_render_pipeline: wgpu::RenderPipeline,
    capture_next_frame: bool,
    #[cfg(not(target_arch = "wasm32"))]
    data_export: mpsc::Sender<MappedTextureView>,
}

impl<'a> State<'a> {
    pub async fn new(
        window: Arc<Window>,
        #[cfg(not(target_arch = "wasm32"))]
        data_export: mpsc::Sender<MappedTextureView>,
    ) -> Self {
        let size = window.inner_size();

        #[cfg(target_arch = "wasm32")]
        {
            web_sys::console::log_1(
                &format!(
                    "Window Inner Size Window Width: {}, Height: {}",
                    size.width, size.height
                )
                .as_str()
                .into(),
            );
        }

        // This is possibly specific to my WoA laptop, the Vulkan driver seems to be broken, so I'm seeing access violations.
        let supported_backends = if cfg!(all(target_arch = "aarch64", target_os = "windows")) {
            wgpu::Backends::DX12
        } else {
            wgpu::Backends::all()
        };

        let instance_desc = InstanceDescriptor {
            backends: supported_backends,
            flags: wgpu::InstanceFlags::from_env_or_default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            backend_options: wgpu::BackendOptions::from_env_or_default(),
            display: None,
        };
        let instance = wgpu::Instance::new(instance_desc);
        let surface = instance
            .create_surface(window.clone())
            .expect("Expected surface from window");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                power_preference: PowerPreference::HighPerformance,
                force_fallback_adapter: false,
            })
            .await
            .expect("Could not create adapter instance!");

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                experimental_features: ExperimentalFeatures::disabled(),
                trace: wgpu::Trace::Off,
            })
            .await
            .expect("Could not get device from adapter!");

        info!("Adapter info: {:?}", adapter.get_info());
        info!("Device info: {:?}", device);

        info!("Supported surface formats:");
        let capabilities = surface.get_capabilities(&adapter);
        capabilities.formats.iter().for_each(|f| {
            info!("Surface format: {:?}", f);
        });

        let surface_format = capabilities.formats[0];
        info!("Selected surface format: {:?}", surface_format);
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![surface_format],
            desired_maximum_frame_latency: 1,
        };

        info!("About to configure surface: {:?}", surface_config);
        surface.configure(&device, &surface_config);

        info!("Surface configured: {:?}", surface_config);
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Texture bind group layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let camera = Camera {
            eye: (0.0, 1.0, 2.0).into(),
            target: (0.0, 0.0, 0.0).into(),
            up: (0.0, 1.0, 0.0).into(),
            aspect: surface_config.width as f32 / surface_config.height as f32,
            fov_y: 45.0,
            z_near: 0.1,
            z_far: 1000.0,
        };

        let camera_controller = CameraController::new(0.2);

        let mut uniforms = Uniforms::new();
        uniforms.update_view_proj(&camera);

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Uniform Buffer"),
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Uniform Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Uniform Bind Group"),
            layout: &uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // Cornflour blue, because I 'member XNA
        let bg_color = rgb_to_normalized(0, 0, 0);

        let light = Light {
            position: [2.0, 2.0, 2.0],
            _padding: 0.0,
            colour: [1.0, 1.0, 1.0],
            _padding2: 0.0,
        };

        let light_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Light buffer"),
            contents: bytemuck::cast_slice(&[light]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let light_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
                label: None,
            });

        let light_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &light_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: light_buffer.as_entire_binding(),
            }],
            label: None,
        });

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[
                    Some(&texture_bind_group_layout),
                    Some(&uniform_bind_group_layout),
                    Some(&light_bind_group_layout),
                ],
                immediate_size: 0,
            });

        let model_loader = ModelLoader::new(&device).await;

        let obj_model = model_loader
            .load(
                &device,
                &queue,
                &texture_bind_group_layout,
                "resources/cube/cube.obj",
            )
            .await
            .unwrap();

        const SPACE_BETWEEN: f32 = 3.0;
        let instances = (0..INSTANCES_PER_ROW)
            .flat_map(|z| {
                (0..INSTANCES_PER_ROW).map(move |x| {
                    let x = SPACE_BETWEEN * (x as f32 - INSTANCES_PER_ROW as f32 / 2.0);
                    let z = SPACE_BETWEEN * (z as f32 - INSTANCES_PER_ROW as f32 / 2.0);

                    let position = cgmath::Vector3 { x, y: 0.0, z } - INSTANCE_DISPLACEMENT;

                    let rotation = if position.is_zero() {
                        cgmath::Quaternion::from_axis_angle(
                            cgmath::Vector3::unit_z(),
                            cgmath::Deg(0.0),
                        )
                    } else {
                        cgmath::Quaternion::from_axis_angle(position.normalize(), cgmath::Deg(45.0))
                    };

                    Instance { position, rotation }
                })
            })
            .collect::<Vec<_>>();

        let instance_data = instances.iter().map(Instance::to_raw).collect::<Vec<_>>();

        info!("Creating instance buffer");
        let instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Instance Buffer"),
            contents: bytemuck::cast_slice(instance_data.as_slice()),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let depth_texture =
            Texture::create_depth_texture(&device, &surface_config, RENDER_SCALE, "Depth Texture");
        let shader_buffer = FileReader::read_file("shaders/shader.wgsl").await;
        let shader_str =
            std::str::from_utf8(shader_buffer.as_slice()).expect("Failed to load shader");

        info!("Creating deferred render pipeline");
        let deferred_render_pipeline = {
            let shader = wgpu::ShaderModuleDescriptor {
                label: Some("Normal Shader"),
                source: wgpu::ShaderSource::Wgsl(shader_str.into()),
            };

            pipeline::create_render_pipeline(
                &device,
                &render_pipeline_layout,
                Some(texture::Texture::DEPTH_FORMAT),
                &[model::ModelVertex::desc(), InstanceRaw::desc()],
                shader,
                &[
                    Some(wgpu::ColorTargetState {
                        format: surface_config.format,
                        blend: Some(wgpu::BlendState {
                            color: wgpu::BlendComponent::REPLACE,
                            alpha: wgpu::BlendComponent::REPLACE,
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: surface_config.format,
                        blend: Some(wgpu::BlendState {
                            color: wgpu::BlendComponent::REPLACE,
                            alpha: wgpu::BlendComponent::REPLACE,
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
                Some("Render Pipeline"),
            )
        };

        info!("Creating light render pipeline");
        let light_render_pipeline = {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Light pipeline layout desc"),
                bind_group_layouts: &[
                    Some(&uniform_bind_group_layout),
                    Some(&light_bind_group_layout),
                ],
                immediate_size: 0,
            });
            let shader_buffer = FileReader::read_file("shaders/light.wgsl").await;
            let shader_str =
                std::str::from_utf8(shader_buffer.as_slice()).expect("Failed to load shader");

            let shader = wgpu::ShaderModuleDescriptor {
                label: Some("Light Shader"),
                source: wgpu::ShaderSource::Wgsl(shader_str.into()),
            };

            pipeline::create_render_pipeline(
                &device,
                &layout,
                Some(texture::Texture::DEPTH_FORMAT),
                &[model::ModelVertex::desc()],
                shader,
                &[
                    Some(wgpu::ColorTargetState {
                        format: surface_config.format,
                        blend: Some(wgpu::BlendState {
                            color: wgpu::BlendComponent::REPLACE,
                            alpha: wgpu::BlendComponent::REPLACE,
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: surface_config.format,
                        blend: Some(wgpu::BlendState {
                            color: wgpu::BlendComponent::REPLACE,
                            alpha: wgpu::BlendComponent::REPLACE,
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
                Some("Light render pipeline"),
            )
        };

        let output_bindgroup_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
                label: None,
            });

        let shader_buffer = FileReader::read_file("shaders/draw_deferred.wgsl").await;
        let shader_str =
            std::str::from_utf8(shader_buffer.as_slice()).expect("Failed to load shader");
        info!("Creating output render pipeline");

        let output_render_pipeline = {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Deferred pipeline layout desc"),
                bind_group_layouts: &[Some(&output_bindgroup_layout)],
                immediate_size: 0,
            });

            let shader = wgpu::ShaderModuleDescriptor {
                label: Some("Output Shader"),
                source: wgpu::ShaderSource::Wgsl(shader_str.into()),
            };

            create_render_pipeline(
                &device,
                &layout,
                None,
                &[QuadVertex::desc()],
                shader,
                &[Some(wgpu::ColorTargetState {
                    format: surface_config.format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent::REPLACE,
                        alpha: wgpu::BlendComponent::REPLACE,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                Some("Output Pipeline"),
            )
        };

        let diffuse_texture = Texture::create_render_texture(
            &device,
            &surface_config,
            RENDER_SCALE,
            "Deferred Diffuse Surface",
        );

        let specular_texture = Texture::create_render_texture(
            &device,
            &surface_config,
            RENDER_SCALE,
            "Deferred Normal Surface",
        );

        let screen_quad = ModelLoader::create_screen_quad_mesh(&device);

        let render_material = {
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &output_bindgroup_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&diffuse_texture.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&diffuse_texture.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&specular_texture.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&specular_texture.sampler),
                    },
                ],
            });

            Material {
                name: String::from("Output Quad Textures"),
                textures: HashMap::from_iter([
                    ("ss_diffuse".to_owned(), diffuse_texture),
                    ("ss_specular".to_owned(), specular_texture),
                ]),
                bind_group,
            }
        };

        info!("State ctor completed");
        Self {
            surface,
            device,
            queue,
            surface_config,
            size,
            bg_color,
            deferred_render_pipeline,
            camera,
            uniforms,
            uniform_buffer,
            uniform_bind_group,
            camera_controller,
            instances,
            instance_buffer,
            depth_texture,
            obj_model,
            light,
            light_buffer,
            light_bind_group,
            light_render_pipeline,
            output_render_pipeline,
            screen_quad,
            render_material,
            capture_next_frame: false,
            #[cfg(not(target_arch = "wasm32"))]
            data_export,
        }
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        self.size = new_size;
        self.surface_config.width = self.size.width;
        self.surface_config.height = self.size.height;
        self.surface.configure(&self.device, &self.surface_config);
        self.depth_texture = Texture::create_depth_texture(
            &self.device,
            &self.surface_config,
            RENDER_SCALE,
            "Depth Texture",
        );
        self.camera.aspect = self.surface_config.width as f32 / self.surface_config.height as f32;

        let diffuse_texture = Texture::create_render_texture(
            &self.device,
            &self.surface_config,
            RENDER_SCALE,
            "Deferred Surface",
        );

        let screen_normal_texture = Texture::create_render_texture(
            &self.device,
            &self.surface_config,
            RENDER_SCALE,
            "Deferred Normal Surface",
        );

        self.render_material = {
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.output_render_pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&diffuse_texture.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&diffuse_texture.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&screen_normal_texture.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&screen_normal_texture.sampler),
                    },
                ],
            });

            Material {
                name: String::from("Output Quad Textures"),
                textures: HashMap::from_iter([
                    ("ss_diffuse".to_owned(), diffuse_texture),
                    ("ss_specular".to_owned(), screen_normal_texture),
                ]),
                bind_group,
            }
        };
    }

    pub fn input(&mut self, event: &WindowEvent) -> bool {
        self.camera_controller.process_inputs(event) || self.process_inputs(event)
    }

    fn process_inputs(&mut self, event: &WindowEvent) -> bool {
        if let WindowEvent::KeyboardInput { event: key, .. } = event {
            if let PhysicalKey::Code(code) = key.physical_key {
                if code == KeyCode::Backspace && key.state.is_pressed() {
                    self.capture_next_frame = true;
                    return true;
                }
            }
        }

        false
    }

    pub fn update(&mut self) {
        let old_position: cgmath::Vector3<_> = self.light.position.into();
        self.light.position =
            (cgmath::Quaternion::from_axis_angle((0.0, 1.0, 0.0).into(), cgmath::Deg(1.0))
                * old_position)
                .into();
        self.queue
            .write_buffer(&self.light_buffer, 0, bytemuck::cast_slice(&[self.light]));

        self.camera_controller.update_camera(&mut self.camera);
        self.uniforms.update_view_proj(&self.camera);
        self.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[self.uniforms]),
        );
    }

    pub fn render(&mut self) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(surface_texture) => surface_texture,
            wgpu::CurrentSurfaceTexture::Lost
            | wgpu::CurrentSurfaceTexture::Outdated
            | wgpu::CurrentSurfaceTexture::Suboptimal(_) => {
                self.resize(self.size);
                return;
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                panic!("Validation error when acquiring next surface texture!")
            }
        };

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        let frame_view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("Render Texture View"),
            format: Some(self.surface_config.format),
            ..Default::default()
        });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Frame render pass"),
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment {
                        view: &self.render_material.textures["ss_diffuse"].view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(self.bg_color),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: &self.render_material.textures["ss_specular"].view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(self.bg_color),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    }),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_texture.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0xFF),
                        store: wgpu::StoreOp::Store,
                    }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            render_pass.set_stencil_reference(32);
            render_pass.set_pipeline(&self.light_render_pipeline);
            render_pass.draw_light_model(
                &self.obj_model,
                &self.uniform_bind_group,
                &self.light_bind_group,
            );

            render_pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            render_pass.set_stencil_reference(64);
            render_pass.set_pipeline(&self.deferred_render_pipeline);
            render_pass.draw_model_instanced(
                &self.obj_model,
                0..self.instances.len() as u32,
                &self.uniform_bind_group,
                &self.light_bind_group,
            );
        }

        if self.capture_next_frame {
            let diffuse_texture = &self.render_material.textures["ss_diffuse"].texture;
            self.read_texture_to_cpu(diffuse_texture, "ss_diffuse");

            let specular_texture = &self.render_material.textures["ss_specular"].texture;
            self.read_texture_to_cpu(specular_texture, "ss_specular");

            self.capture_next_frame = false;
        }

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Post render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &frame_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.bg_color),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            render_pass.set_pipeline(&self.output_render_pipeline);
            render_pass.set_bind_group(0, &self.render_material.bind_group, &[]);

            render_pass.set_vertex_buffer(0, self.screen_quad.vertex_buffer.slice(..));
            render_pass.set_index_buffer(
                self.screen_quad.index_buffer.slice(..),
                wgpu::IndexFormat::Uint32,
            );
            render_pass.draw_indexed(0..self.screen_quad.num_elements, 0, 0..1);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn read_texture_to_cpu(&self, texture: &wgpu::Texture, name: &str) {
        let texture_copy_info = TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        };

        let width = texture.size().width;
        let height = texture.size().height;
        let format = texture.format();
        let bytes_per_pixel = format.block_copy_size(None).unwrap_or(4);

        let unpadded_bytes_per_row = width * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = (unpadded_bytes_per_row + align - 1) & !(align - 1);

        let buffer_size = padded_bytes_per_row as u64 * height as u64;

        let copy_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Copy Buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let copy_layout = wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(padded_bytes_per_row),
            rows_per_image: Some(height),
        };

        let buffer_copy_info = TexelCopyBufferInfo {
            buffer: &copy_buffer,
            layout: copy_layout,
        };

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Texture read encoder"),
            });

        encoder.copy_texture_to_buffer(texture_copy_info, buffer_copy_info, texture.size());

        self.queue.submit(std::iter::once(encoder.finish()));

        let capturable = copy_buffer.clone();
        let sender = self.data_export.clone();
        let name = name.to_owned();
        copy_buffer.map_async(wgpu::MapMode::Read, 0..buffer_size, move |result| {
            if let Ok(()) = result {
                let data = capturable.slice(..).get_mapped_range();
                sender
                    .send(MappedTextureView {
                        data: data.to_vec(),
                        width,
                        height,
                        unpadded_bytes_per_row,
                        padded_bytes_per_row,
                        format,
                        name,
                    })
                    .unwrap();
                drop(data);
                capturable.unmap();
            }
        });
    }

    #[cfg(target_arch = "wasm32")]
    fn read_texture_to_cpu(&self, texture: &wgpu::Texture, name: &str) {
        let texture_copy_info = TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        };

        let width = texture.size().width;
        let height = texture.size().height;
        let format = texture.format();
        let bytes_per_pixel = format.block_copy_size(None).unwrap_or(4);

        let unpadded_bytes_per_row = width * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = (unpadded_bytes_per_row + align - 1) & !(align - 1);

        let buffer_size = padded_bytes_per_row as u64 * height as u64;

        let copy_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Copy Buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let copy_layout = wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(padded_bytes_per_row),
            rows_per_image: Some(height),
        };

        let buffer_copy_info = TexelCopyBufferInfo {
            buffer: &copy_buffer,
            layout: copy_layout,
        };

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Texture read encoder"),
            });

        encoder.copy_texture_to_buffer(texture_copy_info, buffer_copy_info, texture.size());

        self.queue.submit(std::iter::once(encoder.finish()));

        let capturable = copy_buffer.clone();
        let name = name.to_owned();
        copy_buffer.map_async(wgpu::MapMode::Read, 0..buffer_size, move |result| {
            if let Ok(()) = result {
                let raw = capturable.slice(..).get_mapped_range();

                // Strip row padding
                let mut pixels: Vec<u8> =
                    Vec::with_capacity((unpadded_bytes_per_row * height) as usize);
                for row in 0..height {
                    let start = (row * padded_bytes_per_row) as usize;
                    let end = start + unpadded_bytes_per_row as usize;
                    pixels.extend_from_slice(&raw[start..end]);
                }
                drop(raw);
                capturable.unmap();

                // Convert BGRA → RGBA for DX12/Vulkan surface formats
                match format {
                    wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => {
                        for chunk in pixels.chunks_mut(4) {
                            chunk.swap(0, 2);
                        }
                    }
                    _ => {}
                }

                // Encode PNG into an in-memory buffer
                let mut png_bytes: Vec<u8> = Vec::new();
                {
                    use image::ImageEncoder;
                    let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
                    if let Err(e) = encoder.write_image(
                        &pixels,
                        width,
                        height,
                        image::ExtendedColorType::Rgba8,
                    ) {
                        web_sys::console::error_1(
                            &format!("Screenshot PNG encode failed: {e}").into(),
                        );
                        return;
                    }
                }

                // Trigger a browser download
                if let Err(e) = trigger_browser_download(&png_bytes, &format!("{name}.png")) {
                    web_sys::console::error_1(
                        &format!("Screenshot download failed: {e:?}").into(),
                    );
                }
            }
        });
    }
}

#[cfg(target_arch = "wasm32")]
fn trigger_browser_download(data: &[u8], filename: &str) -> Result<(), wasm_bindgen::JsValue> {
    use js_sys::{Array, Uint8Array};
    use wasm_bindgen::JsCast;
    use web_sys::{Blob, BlobPropertyBag, HtmlAnchorElement, Url};

    let uint8_array = Uint8Array::from(data);
    let array = Array::new();
    array.push(&uint8_array.buffer());

    let options = BlobPropertyBag::new();
    options.set_type("image/png");

    let blob = Blob::new_with_u8_array_sequence_and_options(&array, &options)?;
    let url = Url::create_object_url_with_blob(&blob)?;

    let window = web_sys::window().ok_or_else(|| wasm_bindgen::JsValue::from_str("no window"))?;
    let document = window
        .document()
        .ok_or_else(|| wasm_bindgen::JsValue::from_str("no document"))?;

    let anchor: HtmlAnchorElement = document
        .create_element("a")?
        .dyn_into()?;
    anchor.set_href(&url);
    anchor.set_download(filename);
    anchor.click();

    Url::revoke_object_url(&url)?;
    Ok(())
}
