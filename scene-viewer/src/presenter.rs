use scene_renderer::{GpuContext, RendererError};

/// Outcome of [`SurfacePresenter::acquire`].
pub enum Acquire {
    Frame(wgpu::SurfaceTexture),
    /// Nothing to draw this time (zero size, occluded, timeout, reconfigured).
    Skip,
}

/// Host-side surface configuration, acquisition and presentation.
pub struct SurfacePresenter {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    suspended: bool,
}

use scene_renderer::wgpu;

impl SurfacePresenter {
    /// Configure `surface` for `ctx`, preferring the first reported format.
    /// Requests `COPY_SRC` when supported so presented frames can be captured.
    pub fn new(
        ctx: &GpuContext,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) -> Result<Self, RendererError> {
        let caps = surface.get_capabilities(&ctx.adapter);
        let format = *caps
            .formats
            .first()
            .ok_or_else(|| RendererError::Unsupported("surface reports no formats".into()))?;
        let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        if caps.usages.contains(wgpu::TextureUsages::COPY_SRC) {
            usage |= wgpu::TextureUsages::COPY_SRC;
        }
        log::info!("surface format {:?}, usage {:?}", format, usage);
        let config = wgpu::SurfaceConfiguration {
            usage,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![format],
            desired_maximum_frame_latency: 1,
        };
        let suspended = width == 0 || height == 0;
        if !suspended {
            surface.configure(&ctx.device, &config);
        }
        Ok(Self {
            surface,
            config,
            suspended,
        })
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn is_suspended(&self) -> bool {
        self.suspended
    }

    /// Reconfigure for a new size; zero sizes suspend presentation.
    pub fn resize(&mut self, ctx: &GpuContext, width: u32, height: u32) {
        self.suspended = width == 0 || height == 0;
        if self.suspended {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&ctx.device, &self.config);
    }

    pub fn acquire(&mut self, ctx: &GpuContext) -> Result<Acquire, RendererError> {
        if self.suspended {
            return Ok(Acquire::Skip);
        }
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => Ok(Acquire::Frame(frame)),
            wgpu::CurrentSurfaceTexture::Lost
            | wgpu::CurrentSurfaceTexture::Outdated
            | wgpu::CurrentSurfaceTexture::Suboptimal(_) => {
                self.surface.configure(&ctx.device, &self.config);
                Ok(Acquire::Skip)
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                Ok(Acquire::Skip)
            }
            wgpu::CurrentSurfaceTexture::Validation => Err(RendererError::Device(
                "validation error acquiring surface texture".into(),
            )),
        }
    }
}
