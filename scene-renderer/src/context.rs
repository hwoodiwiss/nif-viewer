use crate::RendererError;

/// Backends to request by default. Windows on ARM64 uses DX12 only because
/// its Vulkan drivers have been observed to crash; hosts may override.
pub fn default_backends() -> wgpu::Backends {
    if cfg!(all(target_arch = "aarch64", target_os = "windows")) {
        wgpu::Backends::DX12
    } else {
        wgpu::Backends::all()
    }
}

/// Options for creating a [`GpuContext`].
#[derive(Debug, Clone)]
pub struct ContextDesc {
    pub backends: wgpu::Backends,
    pub power_preference: wgpu::PowerPreference,
    pub required_features: wgpu::Features,
    pub required_limits: wgpu::Limits,
}

impl Default for ContextDesc {
    fn default() -> Self {
        Self {
            backends: default_backends(),
            power_preference: wgpu::PowerPreference::HighPerformance,
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
        }
    }
}

impl ContextDesc {
    /// Create a wgpu instance for these options. Hosts that need a surface
    /// create it from this instance before calling [`GpuContext::new`].
    pub fn create_instance(&self) -> wgpu::Instance {
        wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: self.backends,
            flags: wgpu::InstanceFlags::from_env_or_default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            backend_options: wgpu::BackendOptions::from_env_or_default(),
            display: None,
        })
    }
}

/// Adapter, device and queue shared by renderers and presenters.
///
/// Cloning is cheap: wgpu handles are reference counted.
#[derive(Clone, Debug)]
pub struct GpuContext {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

/// What the device can do, for capability negotiation.
#[derive(Debug, Clone)]
pub struct Capabilities {
    pub adapter: wgpu::AdapterInfo,
    pub limits: wgpu::Limits,
    pub features: wgpu::Features,
}

impl GpuContext {
    /// Request an adapter (compatible with `surface` when given) and device.
    pub async fn new(
        instance: wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        desc: &ContextDesc,
    ) -> Result<Self, RendererError> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: desc.power_preference,
                compatible_surface: surface,
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| RendererError::NoAdapter(e.to_string()))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("scene-renderer device"),
                required_features: desc.required_features,
                required_limits: desc.required_limits.clone(),
                memory_hints: wgpu::MemoryHints::Performance,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| RendererError::Device(e.to_string()))?;
        log::info!("adapter: {:?}", adapter.get_info());
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }

    /// Context for offscreen rendering, with no surface.
    pub async fn new_offscreen(desc: &ContextDesc) -> Result<Self, RendererError> {
        Self::new(desc.create_instance(), None, desc).await
    }

    /// Wrap an existing device. It must satisfy [`wgpu::Limits::default`]
    /// downlevel WebGPU limits and come from the same wgpu version.
    pub fn from_parts(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
    ) -> Self {
        Self {
            instance,
            adapter,
            device,
            queue,
        }
    }

    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            adapter: self.adapter.get_info(),
            limits: self.device.limits(),
            features: self.device.features(),
        }
    }

    pub fn create_encoder(&self, label: &str) -> wgpu::CommandEncoder {
        self.device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) })
    }

    pub fn submit(&self, encoder: wgpu::CommandEncoder) -> wgpu::SubmissionIndex {
        self.queue.submit(std::iter::once(encoder.finish()))
    }
}
