pub trait Bindable {
    fn layout_entries() -> Vec<wgpu::BindGroupLayoutEntry>;
    fn bind_group_entries(&self) -> Vec<wgpu::BindGroupEntry<'_>>;
}

pub struct Binder<T: Bindable> {
    pub(crate) layout: wgpu::BindGroupLayout,
    _marker: std::marker::PhantomData<T>,
}

impl<T: Bindable> Binder<T> {
    pub fn new(device: &wgpu::Device, label: Option<&str>) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label,
            entries: &T::layout_entries(),
        });
        Self {
            layout,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn create_bind_group(
        &self,
        data: &T,
        device: &wgpu::Device,
        label: Option<&str>,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label,
            layout: &self.layout,
            entries: &data.bind_group_entries(),
        })
    }
}

pub fn create_render_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    depth_format: Option<wgpu::TextureFormat>,
    vertex_layouts: &[wgpu::VertexBufferLayout],
    shader: wgpu::ShaderModuleDescriptor,
    targets: &[Option<wgpu::ColorTargetState>],
    label: Option<&str>,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(shader);

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label,
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex_main"),
            buffers: vertex_layouts,
            compilation_options: wgpu::PipelineCompilationOptions {
                ..Default::default()
            },
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fragment_main"),
            targets,
            compilation_options: wgpu::PipelineCompilationOptions {
                ..Default::default()
            },
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: depth_format.map(|format| wgpu::DepthStencilState {
            format,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: wgpu::StencilState {
                front: wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::Always,
                    fail_op: wgpu::StencilOperation::Keep,
                    depth_fail_op: wgpu::StencilOperation::Keep,
                    pass_op: wgpu::StencilOperation::Replace,
                },
                back: wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::Never,
                    fail_op: wgpu::StencilOperation::Keep,
                    depth_fail_op: wgpu::StencilOperation::Keep,
                    pass_op: wgpu::StencilOperation::Keep,
                },
                read_mask: 0xFF,
                write_mask: 0xFF,
            },
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        cache: None,
        multiview_mask: None,
    })
}

pub fn create_compute_pipeline(
    device: &wgpu::Device,
    bind_group_layouts: &[Option<&wgpu::BindGroupLayout>],
    shader: wgpu::ShaderModuleDescriptor,
    label: Option<&str>,
) -> wgpu::ComputePipeline {
    let shader = device.create_shader_module(shader);

    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label,
        bind_group_layouts,
        immediate_size: 0,
    });

    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("main"),
        compilation_options: wgpu::PipelineCompilationOptions {
            ..Default::default()
        },
        cache: None,
    })
}
