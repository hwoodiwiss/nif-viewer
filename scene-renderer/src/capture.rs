use crate::RendererError;

/// Pixels read back from a render target: RGBA8, unpadded rows, top-left origin.
///
/// BGRA targets are swizzled to RGBA. Float (linear) targets are converted to
/// 8-bit sRGB so they can be viewed or compared like the presented output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedImage {
    pub label: String,
    /// Frame counter value from the [`crate::FrameReceipt`] the copy was encoded with.
    pub frame: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl CapturedImage {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let o = ((y * self.width + x) * 4) as usize;
        [
            self.rgba[o],
            self.rgba[o + 1],
            self.rgba[o + 2],
            self.rgba[o + 3],
        ]
    }
}

/// A texture-to-buffer copy encoded in a frame's command encoder. Call
/// [`PendingCapture::map`] only after that encoder has been submitted.
#[must_use = "a capture does nothing until mapped after submission"]
pub struct PendingCapture {
    pub(crate) buffer: wgpu::Buffer,
    pub(crate) label: String,
    pub(crate) frame: u64,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) padded_bytes_per_row: u32,
    pub(crate) format: wgpu::TextureFormat,
}

pub(crate) fn supported(format: wgpu::TextureFormat) -> bool {
    use wgpu::TextureFormat::*;
    matches!(
        format,
        Rgba8Unorm | Rgba8UnormSrgb | Bgra8Unorm | Bgra8UnormSrgb | Rgba16Float
    )
}

impl PendingCapture {
    pub(crate) fn encode(
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        label: &str,
        frame: u64,
    ) -> Result<Self, RendererError> {
        let format = texture.format();
        if !supported(format) {
            return Err(RendererError::Capture(format!(
                "unsupported capture format {:?}",
                format
            )));
        }
        if !texture.usage().contains(wgpu::TextureUsages::COPY_SRC) {
            return Err(RendererError::Capture(
                "capture texture lacks COPY_SRC usage".into(),
            ));
        }
        let (width, height) = (texture.width(), texture.height());
        let bpp = format.block_copy_size(None).unwrap_or(4);
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = (width * bpp).div_ceil(align) * align;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture readback"),
            size: padded_bytes_per_row as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            texture.size(),
        );
        Ok(Self {
            buffer,
            label: label.to_string(),
            frame,
            width,
            height,
            padded_bytes_per_row,
            format,
        })
    }

    /// Start mapping; `on_ready` runs from a later device poll (native) or the
    /// browser event loop (web) with the converted image or the map error.
    pub fn map(
        self,
        on_ready: impl FnOnce(Result<CapturedImage, RendererError>) + wgpu::WasmNotSend + 'static,
    ) {
        let buffer = self.buffer.clone();
        let slice = buffer.slice(..);
        let mapped = buffer.clone();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let image = match result {
                Ok(()) => {
                    let data = mapped.slice(..).get_mapped_range();
                    let image = self.convert(&data);
                    drop(data);
                    mapped.unmap();
                    Ok(image)
                }
                Err(e) => Err(RendererError::Capture(e.to_string())),
            };
            on_ready(image);
        });
    }

    fn convert(&self, data: &[u8]) -> CapturedImage {
        let bpp = self.format.block_copy_size(None).unwrap_or(4) as usize;
        let mut rgba = Vec::with_capacity((self.width * self.height * 4) as usize);
        for row in 0..self.height as usize {
            let start = row * self.padded_bytes_per_row as usize;
            let line = &data[start..start + self.width as usize * bpp];
            match self.format {
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => {
                    for px in line.as_chunks::<4>().0 {
                        rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
                    }
                }
                wgpu::TextureFormat::Rgba16Float => {
                    for px in line.as_chunks::<8>().0 {
                        let c = |i: usize| {
                            half::f16::from_le_bytes([px[i * 2], px[i * 2 + 1]]).to_f32()
                        };
                        rgba.extend_from_slice(&[
                            linear_to_srgb8(c(0)),
                            linear_to_srgb8(c(1)),
                            linear_to_srgb8(c(2)),
                            (c(3).clamp(0.0, 1.0) * 255.0).round() as u8,
                        ]);
                    }
                }
                _ => rgba.extend_from_slice(line),
            }
        }
        CapturedImage {
            label: self.label.clone(),
            frame: self.frame,
            width: self.width,
            height: self.height,
            rgba,
        }
    }
}

fn linear_to_srgb8(v: f32) -> u8 {
    let v = if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let s = if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_encoding_endpoints() {
        assert_eq!(linear_to_srgb8(0.0), 0);
        assert_eq!(linear_to_srgb8(1.0), 255);
        assert_eq!(linear_to_srgb8(0.5), 188);
        assert_eq!(linear_to_srgb8(f32::NAN), 0);
    }
}
