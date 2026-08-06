use anyhow::*;
use std::collections::HashMap;
use std::iter::FromIterator;
use std::{ops::Range, path::Path};

use crate::file_reader::FileReader;
use crate::pipeline;
use crate::texture::Texture;
use crate::vertex::Vertex;

use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct QuadVertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
}

impl Vertex for QuadVertex {
    fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<QuadVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 1,
                },
            ],
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ModelVertex {
    position: [f32; 3],
    tex_coords: [f32; 2],
    normal: [f32; 3],
    tangent: [f32; 3],
    bitangent: [f32; 3],
    padding: [u32; 2],
}

impl Vertex for ModelVertex {
    fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ModelVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: std::mem::size_of::<[f32; 5]>() as wgpu::BufferAddress,
                    shader_location: 2,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: std::mem::size_of::<[f32; 8]>() as wgpu::BufferAddress,
                    shader_location: 3,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: std::mem::size_of::<[f32; 11]>() as wgpu::BufferAddress,
                    shader_location: 4,
                },
            ],
        }
    }
}

pub struct Material {
    #[allow(dead_code)]
    pub name: String,
    pub textures: HashMap<String, Texture>,
    pub bind_group: wgpu::BindGroup,
}

pub struct Mesh {
    #[allow(dead_code)]
    pub name: String,
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub num_elements: u32,
    pub material: usize,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct ComputeInfo {
    num_vertices: u32,
    num_indices: u32,
}

struct BitangentComputeBinding {
    src_vertex_buffer: wgpu::Buffer,
    dst_vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    info_buffer: wgpu::Buffer,
    compute_info: ComputeInfo,
}

impl pipeline::Bindable for BitangentComputeBinding {
    fn layout_entries() -> Vec<wgpu::BindGroupLayoutEntry> {
        vec![
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ]
    }

    fn bind_group_entries(&self) -> Vec<wgpu::BindGroupEntry<'_>> {
        vec![
            //Src Verts
            wgpu::BindGroupEntry {
                binding: 0,
                resource: self.src_vertex_buffer.as_entire_binding(),
            },
            //Dst Verts
            wgpu::BindGroupEntry {
                binding: 1,
                resource: self.dst_vertex_buffer.as_entire_binding(),
            },
            //Index Buffer
            wgpu::BindGroupEntry {
                binding: 2,
                resource: self.index_buffer.as_entire_binding(),
            },
            //Compute info buffer
            wgpu::BindGroupEntry {
                binding: 3,
                resource: self.info_buffer.as_entire_binding(),
            },
        ]
    }
}

pub struct Model {
    pub meshes: Vec<Mesh>,
    pub materials: Vec<Material>,
}

pub struct ModelLoader {
    binder: pipeline::Binder<BitangentComputeBinding>,
    pipeline: wgpu::ComputePipeline,
}

impl ModelLoader {
    pub async fn new(device: &wgpu::Device) -> Self {
        let binder = pipeline::Binder::new(device, Some("ModelLoader Binder"));

        let shader_buffer = FileReader::read_file("shaders/compute_bitangents.wgsl").await;
        let shader_str =
            std::str::from_utf8(shader_buffer.as_slice()).expect("Failed to load shader");

        let shader = wgpu::ShaderModuleDescriptor {
            source: wgpu::ShaderSource::Wgsl(shader_str.into()),
            label: Some("Bitangent Compute Shader Module"),
        };

        let pipeline = pipeline::create_compute_pipeline(
            device,
            &[Some(&binder.layout)],
            shader,
            Some("ModelLoader Compute Pipeline"),
        );

        Self { binder, pipeline }
    }

    pub async fn load<P: AsRef<Path>>(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        path: P,
    ) -> Result<Model> {
        let path = path.as_ref();
        let resource_base = path.parent().expect("Could not determine model base path");
        let obj_data =
            FileReader::read_file(path.to_str().expect("Could not convert model path to &str"))
                .await;
        let (obj_models, obj_materials) = tobj::futures::load_obj_buf(
            &mut obj_data.as_slice(),
            &tobj::LoadOptions {
                triangulate: true,
                single_index: true,
                ..Default::default()
            },
            async |path| {
                let mtl_data = FileReader::read_file(
                    resource_base
                        .join(path)
                        .to_str()
                        .expect("Could not convert material path to &str"),
                )
                .await;
                tobj::load_mtl_buf(&mut mtl_data.as_slice())
            },
        )
        .await?;

        let obj_materials = obj_materials?;

        let mut materials = Vec::new();

        for mat in obj_materials {
            let diffuse_path = &mat.diffuse_texture;
            let diffuse_path = diffuse_path
                .clone()
                .expect("Material has no diffuse texture");
            let diffuse_texture = Texture::load(
                device,
                queue,
                resource_base
                    .join(diffuse_path.clone())
                    .to_str()
                    .unwrap_or_else(|| {
                        panic!(
                            "{}",
                            ("Could not convert diffuse path to &str: ".to_owned() + &diffuse_path)
                        )
                    }),
                false,
            )
            .await?;

            let normal_path = &mat.normal_texture;
            let normal_path = normal_path
                .clone()
                .expect("Material has no normal texture!");
            let normal_texture = Texture::load(
                device,
                queue,
                resource_base
                    .join(normal_path.clone())
                    .to_str()
                    .unwrap_or_else(|| {
                        panic!(
                            "{}",
                            ("Could not convert normal path to &str: ".to_owned() + &normal_path)
                        )
                    }),
                true,
            )
            .await?;

            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout,
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
                        resource: wgpu::BindingResource::TextureView(&normal_texture.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&normal_texture.sampler),
                    },
                ],
            });

            materials.push(Material {
                name: mat.name,
                textures: HashMap::from_iter([
                    ("diffuse".to_owned(), diffuse_texture),
                    ("normal".to_owned(), normal_texture),
                ]),
                bind_group,
            })
        }

        let mut meshes = Vec::new();

        for model in obj_models {
            let mut vertices = Vec::with_capacity(model.mesh.positions.len() / 3);
            for i in 0..model.mesh.positions.len() / 3 {
                vertices.push(ModelVertex {
                    position: [
                        model.mesh.positions[i * 3],
                        model.mesh.positions[i * 3 + 1],
                        model.mesh.positions[i * 3 + 2],
                    ],
                    tex_coords: [model.mesh.texcoords[i * 2], model.mesh.texcoords[i * 2 + 1]],
                    normal: [
                        model.mesh.normals[i * 3],
                        model.mesh.normals[i * 3 + 1],
                        model.mesh.normals[i * 3 + 2],
                    ],
                    tangent: [0.0; 3],
                    bitangent: [0.0; 3],
                    padding: [0u32; 2],
                });
            }

            let indices = &model.mesh.indices;

            let src_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("{:?} Compute Src Vertex Buffer", path)),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::STORAGE,
            });

            let dst_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("{:?} Compute Dst Vertex Buffer", path)),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE,
            });

            let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("{:?} Index Buffer", path)),
                contents: bytemuck::cast_slice(&model.mesh.indices),
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::STORAGE,
            });

            let compute_info = ComputeInfo {
                num_vertices: vertices.len() as _,
                num_indices: indices.len() as _,
            };

            let info_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("{:?} Compute Info Buffer", path)),
                contents: bytemuck::cast_slice(&[compute_info]),
                usage: wgpu::BufferUsages::UNIFORM,
            });

            let binding = BitangentComputeBinding {
                src_vertex_buffer,
                dst_vertex_buffer,
                index_buffer,
                info_buffer,
                compute_info,
            };

            let calc_bind_group = self.binder.create_bind_group(
                &binding,
                device,
                Some("Bitangent Compute Binding Group"),
            );
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Tangent and Bitangent compute encoder"),
            });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("Compute Pass"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &calc_bind_group, &[]);
                pass.dispatch_workgroups(binding.compute_info.num_vertices, 1, 1);
            }
            queue.submit(std::iter::once(encoder.finish()));
            if let Err(e) = device.poll(wgpu::PollType::Poll) {
                return Err(anyhow!(
                    "Error during compute bitangent calculation: {:?}",
                    e
                ));
            }

            meshes.push(Mesh {
                name: model.name,
                vertex_buffer: binding.dst_vertex_buffer,
                index_buffer: binding.index_buffer,
                num_elements: binding.compute_info.num_indices,
                material: model.mesh.material_id.unwrap_or(0),
            });
        }

        Ok(Model { meshes, materials })
    }

    pub fn create_screen_quad_mesh(device: &wgpu::Device) -> Mesh {
        let quad_verts = [
            QuadVertex {
                position: [-1.0, 1.0],
                tex_coords: [0.0, 0.0],
            },
            QuadVertex {
                position: [1.0, 1.0],
                tex_coords: [1.0, 0.0],
            },
            QuadVertex {
                position: [1.0, -1.0],
                tex_coords: [1.0, 1.0],
            },
            QuadVertex {
                position: [-1.0, -1.0],
                tex_coords: [0.0, 1.0],
            },
        ];

        let quad_indices = [0u32, 1u32, 2u32, 0u32, 2u32, 3u32];

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Output Vertex Buffer"),
            contents: bytemuck::cast_slice(&quad_verts),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE,
        });

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Output Index Buffer"),
            contents: bytemuck::cast_slice(&quad_indices),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::STORAGE,
        });

        Mesh {
            name: String::from("Output Quad"),
            vertex_buffer,
            index_buffer,
            num_elements: quad_indices.len() as u32,
            material: 0,
        }
    }
}

pub trait DrawModel<'a, 'b>
where
    'b: 'a,
{
    #[allow(dead_code)]
    fn draw_mesh(
        &mut self,
        mesh: &'b Mesh,
        material: &'b Material,
        uniforms: &'b wgpu::BindGroup,
        light: &'b wgpu::BindGroup,
    );
    fn draw_mesh_instanced(
        &mut self,
        mesh: &'b Mesh,
        material: &'b Material,
        instances: Range<u32>,
        uniforms: &'b wgpu::BindGroup,
        light: &'b wgpu::BindGroup,
    );

    #[allow(dead_code)]
    fn draw_model(
        &mut self,
        model: &'b Model,
        uniforms: &'b wgpu::BindGroup,
        light: &'b wgpu::BindGroup,
    );
    fn draw_model_instanced(
        &mut self,
        model: &'b Model,
        instances: Range<u32>,
        uniforms: &'b wgpu::BindGroup,
        light: &'b wgpu::BindGroup,
    );
}

impl<'a, 'b> DrawModel<'a, 'b> for wgpu::RenderPass<'a>
where
    'b: 'a,
{
    fn draw_mesh(
        &mut self,
        mesh: &'b Mesh,
        material: &'b Material,
        uniforms: &'b wgpu::BindGroup,
        light: &'b wgpu::BindGroup,
    ) {
        self.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
        self.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        self.set_bind_group(0, &material.bind_group, &[]);
        self.set_bind_group(1, uniforms, &[]);
        self.set_bind_group(2, light, &[]);
        self.draw_mesh_instanced(mesh, material, 0..1, uniforms, light);
    }

    fn draw_mesh_instanced(
        &mut self,
        mesh: &'b Mesh,
        material: &'b Material,
        instances: Range<u32>,
        uniforms: &'b wgpu::BindGroup,
        light: &'b wgpu::BindGroup,
    ) {
        self.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
        self.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        self.set_bind_group(0, &material.bind_group, &[]);
        self.set_bind_group(1, uniforms, &[]);
        self.set_bind_group(2, light, &[]);
        self.draw_indexed(0..mesh.num_elements, 0, instances);
    }

    fn draw_model(
        &mut self,
        model: &'b Model,
        uniforms: &'b wgpu::BindGroup,
        light: &'b wgpu::BindGroup,
    ) {
        self.draw_model_instanced(model, 0..1, uniforms, light);
    }

    fn draw_model_instanced(
        &mut self,
        model: &'b Model,
        instances: Range<u32>,
        uniforms: &'b wgpu::BindGroup,
        light: &'b wgpu::BindGroup,
    ) {
        for mesh in &model.meshes {
            let material = &model.materials[mesh.material];
            self.draw_mesh_instanced(mesh, material, instances.clone(), uniforms, light);
        }
    }
}

pub trait DrawLight<'a, 'b>
where
    'b: 'a,
{
    #[allow(dead_code)]
    fn draw_light_mesh(
        &mut self,
        mesh: &'b Mesh,
        uniforms: &'b wgpu::BindGroup,
        light: &'b wgpu::BindGroup,
    );
    fn draw_light_mesh_instanced(
        &mut self,
        mesh: &'b Mesh,
        instances: Range<u32>,
        uniforms: &'b wgpu::BindGroup,
        light: &'b wgpu::BindGroup,
    );

    fn draw_light_model(
        &mut self,
        model: &'b Model,
        uniforms: &'b wgpu::BindGroup,
        light: &'b wgpu::BindGroup,
    );
    fn draw_light_model_instanced(
        &mut self,
        model: &'b Model,
        instances: Range<u32>,
        uniforms: &'b wgpu::BindGroup,
        light: &'b wgpu::BindGroup,
    );
}

impl<'a, 'b> DrawLight<'a, 'b> for wgpu::RenderPass<'a>
where
    'b: 'a,
{
    fn draw_light_mesh(
        &mut self,
        mesh: &'b Mesh,
        uniforms: &'b wgpu::BindGroup,
        light: &'b wgpu::BindGroup,
    ) {
        self.draw_light_mesh_instanced(mesh, 0..1, uniforms, light);
    }

    fn draw_light_mesh_instanced(
        &mut self,
        mesh: &'b Mesh,
        instances: Range<u32>,
        uniforms: &'b wgpu::BindGroup,
        light: &'b wgpu::BindGroup,
    ) {
        self.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
        self.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        self.set_bind_group(0, uniforms, &[]);
        self.set_bind_group(1, light, &[]);
        self.draw_indexed(0..mesh.num_elements, 0, instances);
    }

    fn draw_light_model(
        &mut self,
        model: &'b Model,
        uniforms: &'b wgpu::BindGroup,
        light: &'b wgpu::BindGroup,
    ) {
        self.draw_light_model_instanced(model, 0..1, uniforms, light);
    }

    fn draw_light_model_instanced(
        &mut self,
        model: &'b Model,
        instances: Range<u32>,
        uniforms: &'b wgpu::BindGroup,
        light: &'b wgpu::BindGroup,
    ) {
        for mesh in &model.meshes {
            self.draw_light_mesh_instanced(mesh, instances.clone(), uniforms, light);
        }
    }
}
