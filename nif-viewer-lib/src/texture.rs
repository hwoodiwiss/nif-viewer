use anyhow::*;
use image::GenericImageView;

use crate::file_reader::FileReader;

pub struct Texture {
    #[allow(dead_code)]
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
}

impl Texture {
    pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;

    pub fn create_depth_texture(
        device: &wgpu::Device,
        surface_config: &wgpu::SurfaceConfiguration,
        render_scale: f32,
        label: &str,
    ) -> Self {
        let size = wgpu::Extent3d {
            width: (surface_config.width as f32 * render_scale) as u32,
            height: (surface_config.height as f32 * render_scale) as u32,
            depth_or_array_layers: 1,
        };

        let desc = wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[Self::DEPTH_FORMAT],
        };

        let texture = device.create_texture(&desc);

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Depth Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            lod_min_clamp: 0.0,
            lod_max_clamp: 100.0,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });

        Self {
            texture,
            view,
            sampler,
        }
    }

    pub fn create_render_texture(
        device: &wgpu::Device,
        surface_config: &wgpu::SurfaceConfiguration,
        render_scale: f32,
        label: &str,
    ) -> Self {
        let size = wgpu::Extent3d {
            width: (surface_config.width as f32 * render_scale) as u32,
            height: (surface_config.height as f32 * render_scale) as u32,
            depth_or_array_layers: 1,
        };

        let desc = wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: surface_config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[surface_config.format],
        };

        let texture = device.create_texture(&desc);

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        Self {
            texture,
            view,
            sampler,
        }
    }

    #[allow(dead_code)]
    pub fn from_bytes(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bytes: &[u8],
        label: &str,
        is_normal_map: bool,
    ) -> Result<Self> {
        let img = image::load_from_memory(bytes)?;
        Self::from_image(device, queue, &img, Some(label), is_normal_map)
    }

    pub async fn load(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        path: &str,
        is_normal_map: bool,
    ) -> Result<Self> {
        let label = Some(path);

        let img_buffer = FileReader::read_file(path).await;
        let img = if path.contains(".tga") {
            image::load_from_memory_with_format(&img_buffer, image::ImageFormat::Tga)?
        } else {
            image::load_from_memory(&img_buffer)?
        };
        Self::from_image(device, queue, &img, label, is_normal_map)
    }

    pub fn from_image(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        img: &image::DynamicImage,
        label: Option<&str>,
        is_normal_map: bool,
    ) -> Result<Self> {
        let rgba = img.to_rgba8();
        let dimensions = img.dimensions();
        Self::from_rgba8(device, queue, &rgba, dimensions, label, is_normal_map)
    }

    /// Decode a DDS file (BC1/BC2/BC3/BC4/BC5/BC7 or uncompressed RGBA8/BGRA8)
    /// to RGBA8 on the CPU and upload the top mip level as a texture.
    pub fn from_dds_bytes(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bytes: &[u8],
        label: &str,
        is_normal_map: bool,
    ) -> Result<Self> {
        let (rgba, width, height) = decode_dds_rgba8(bytes, is_normal_map)?;
        Self::from_rgba8(
            device,
            queue,
            &rgba,
            (width, height),
            Some(label),
            is_normal_map,
        )
    }

    /// Create a texture from raw RGBA8 pixel data.
    pub fn from_rgba8(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rgba: &[u8],
        dimensions: (u32, u32),
        label: Option<&str>,
        is_normal_map: bool,
    ) -> Result<Self> {
        let texture_size = wgpu::Extent3d {
            width: dimensions.0,
            height: dimensions.1,
            depth_or_array_layers: 1,
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label,
            size: texture_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: if is_normal_map {
                wgpu::TextureFormat::Rgba8Unorm
            } else {
                wgpu::TextureFormat::Rgba8UnormSrgb
            },
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
        });

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * dimensions.0),
                rows_per_image: Some(dimensions.1),
            },
            texture_size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        Ok(Texture {
            texture,
            view,
            sampler,
        })
    }
}

/// Which BC codec / uncompressed layout a DDS file uses.
enum DdsCodec {
    Bc1,
    Bc2,
    Bc3,
    Bc4,
    /// BC4 with signed (SNORM) endpoints; decoded as unsigned then remapped.
    Bc4Snorm,
    Bc5,
    /// BC5 with signed (SNORM) endpoints; decoded as unsigned then remapped.
    Bc5Snorm,
    Bc7,
    Rgba8,
    Bgra8,
}

const fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*s)
}

/// Decode the top mip level of a DDS file to tightly-packed RGBA8.
///
/// For BC5 normal maps the Z channel is reconstructed as
/// `z = sqrt(1 - x^2 - y^2)` mapped back into [0, 255].
pub fn decode_dds_rgba8(bytes: &[u8], is_normal_map: bool) -> Result<(Vec<u8>, u32, u32)> {
    use ddsfile::{D3DFormat, Dds, DxgiFormat};

    let dds = Dds::read(bytes).map_err(|e| anyhow!("failed to parse DDS: {e}"))?;
    let width = dds.get_width();
    let height = dds.get_height();
    if width == 0 || height == 0 {
        return Err(anyhow!("DDS has zero dimensions"));
    }

    let codec = if let Some(fmt) = dds.get_dxgi_format() {
        match fmt {
            DxgiFormat::BC1_UNorm | DxgiFormat::BC1_UNorm_sRGB | DxgiFormat::BC1_Typeless => {
                DdsCodec::Bc1
            }
            DxgiFormat::BC2_UNorm | DxgiFormat::BC2_UNorm_sRGB | DxgiFormat::BC2_Typeless => {
                DdsCodec::Bc2
            }
            DxgiFormat::BC3_UNorm | DxgiFormat::BC3_UNorm_sRGB | DxgiFormat::BC3_Typeless => {
                DdsCodec::Bc3
            }
            DxgiFormat::BC4_UNorm | DxgiFormat::BC4_Typeless => DdsCodec::Bc4,
            DxgiFormat::BC4_SNorm => DdsCodec::Bc4Snorm,
            DxgiFormat::BC5_UNorm | DxgiFormat::BC5_Typeless => DdsCodec::Bc5,
            DxgiFormat::BC5_SNorm => DdsCodec::Bc5Snorm,
            DxgiFormat::BC7_UNorm | DxgiFormat::BC7_UNorm_sRGB | DxgiFormat::BC7_Typeless => {
                DdsCodec::Bc7
            }
            DxgiFormat::R8G8B8A8_UNorm
            | DxgiFormat::R8G8B8A8_UNorm_sRGB
            | DxgiFormat::R8G8B8A8_Typeless => DdsCodec::Rgba8,
            DxgiFormat::B8G8R8A8_UNorm
            | DxgiFormat::B8G8R8A8_UNorm_sRGB
            | DxgiFormat::B8G8R8A8_Typeless => DdsCodec::Bgra8,
            other => return Err(anyhow!("unsupported DDS DXGI format: {other:?}")),
        }
    } else if let Some(fmt) = dds.get_d3d_format() {
        match fmt {
            D3DFormat::DXT1 => DdsCodec::Bc1,
            D3DFormat::DXT2 | D3DFormat::DXT3 => DdsCodec::Bc2,
            D3DFormat::DXT4 | D3DFormat::DXT5 => DdsCodec::Bc3,
            D3DFormat::A8B8G8R8 => DdsCodec::Rgba8,
            D3DFormat::A8R8G8B8 => DdsCodec::Bgra8,
            other => return Err(anyhow!("unsupported DDS D3D format: {other:?}")),
        }
    } else if let Some(fc) = &dds.header.spf.fourcc {
        // Legacy FourCC codes that ddsfile's D3DFormat enum does not map.
        match fc.0 {
            x if x == fourcc(b"ATI1") || x == fourcc(b"BC4U") => DdsCodec::Bc4,
            x if x == fourcc(b"BC4S") => DdsCodec::Bc4Snorm,
            x if x == fourcc(b"ATI2") || x == fourcc(b"BC5U") => DdsCodec::Bc5,
            x if x == fourcc(b"BC5S") => DdsCodec::Bc5Snorm,
            other => return Err(anyhow!("unsupported DDS FourCC: {other:#010x}")),
        }
    } else {
        return Err(anyhow!("DDS has no recognizable pixel format"));
    };

    let data = dds
        .get_data(0)
        .map_err(|e| anyhow!("failed to get DDS data: {e}"))?;

    let (w, h) = (width as usize, height as usize);
    let mut out = vec![0u8; w * h * 4];

    match codec {
        DdsCodec::Rgba8 | DdsCodec::Bgra8 => {
            let needed = w * h * 4;
            if data.len() < needed {
                return Err(anyhow!("DDS uncompressed data too small"));
            }
            out.copy_from_slice(&data[..needed]);
            if matches!(codec, DdsCodec::Bgra8) {
                for px in out.chunks_exact_mut(4) {
                    px.swap(0, 2);
                }
            }
        }
        DdsCodec::Bc4 | DdsCodec::Bc4Snorm => {
            let signed = matches!(codec, DdsCodec::Bc4Snorm);
            let mut gray = vec![0u8; w * h];
            for_each_block(data, w, h, 8, |bx, by, block| {
                let mut tmp = [0u8; 16];
                if signed {
                    let mut block_u = [0u8; 8];
                    block_u.copy_from_slice(block);
                    // Remap SNORM endpoints to UNORM before decoding.
                    block_u[0] = block_u[0].wrapping_add(128);
                    block_u[1] = block_u[1].wrapping_add(128);
                    bcdec_rs::bc4(&block_u, &mut tmp, 4);
                } else {
                    bcdec_rs::bc4(block, &mut tmp, 4);
                }
                write_block(&mut gray, w, h, bx, by, 1, &tmp);
            })?;
            for (i, px) in out.chunks_exact_mut(4).enumerate() {
                px[0] = gray[i];
                px[1] = gray[i];
                px[2] = gray[i];
                px[3] = 255;
            }
        }
        DdsCodec::Bc5 | DdsCodec::Bc5Snorm => {
            let signed = matches!(codec, DdsCodec::Bc5Snorm);
            let mut rg = vec![0u8; w * h * 2];
            for_each_block(data, w, h, 16, |bx, by, block| {
                let mut tmp = [0u8; 32];
                if signed {
                    let mut block_u = [0u8; 16];
                    block_u.copy_from_slice(block);
                    // Two BC4 sub-blocks; remap SNORM endpoints to UNORM.
                    for off in [0usize, 8] {
                        block_u[off] = block_u[off].wrapping_add(128);
                        block_u[off + 1] = block_u[off + 1].wrapping_add(128);
                    }
                    bcdec_rs::bc5(&block_u, &mut tmp, 8);
                } else {
                    bcdec_rs::bc5(block, &mut tmp, 8);
                }
                write_block(&mut rg, w, h, bx, by, 2, &tmp);
            })?;
            for (i, px) in out.chunks_exact_mut(4).enumerate() {
                let r = rg[i * 2];
                let g = rg[i * 2 + 1];
                px[0] = r;
                px[1] = g;
                px[2] = if is_normal_map {
                    // Reconstruct Z = sqrt(1 - x^2 - y^2) in [-1,1] space.
                    let x = r as f32 / 255.0 * 2.0 - 1.0;
                    let y = g as f32 / 255.0 * 2.0 - 1.0;
                    let z = (1.0 - x * x - y * y).max(0.0).sqrt();
                    ((z * 0.5 + 0.5) * 255.0).round() as u8
                } else {
                    0
                };
                px[3] = 255;
            }
        }
        DdsCodec::Bc1 => {
            for_each_block(data, w, h, 8, |bx, by, block| {
                let mut tmp = [0u8; 64];
                bcdec_rs::bc1(block, &mut tmp, 16);
                write_block(&mut out, w, h, bx, by, 4, &tmp);
            })?;
        }
        DdsCodec::Bc2 => {
            for_each_block(data, w, h, 16, |bx, by, block| {
                let mut tmp = [0u8; 64];
                bcdec_rs::bc2(block, &mut tmp, 16);
                write_block(&mut out, w, h, bx, by, 4, &tmp);
            })?;
        }
        DdsCodec::Bc3 => {
            for_each_block(data, w, h, 16, |bx, by, block| {
                let mut tmp = [0u8; 64];
                bcdec_rs::bc3(block, &mut tmp, 16);
                write_block(&mut out, w, h, bx, by, 4, &tmp);
            })?;
        }
        DdsCodec::Bc7 => {
            for_each_block(data, w, h, 16, |bx, by, block| {
                let mut tmp = [0u8; 64];
                bcdec_rs::bc7(block, &mut tmp, 16);
                write_block(&mut out, w, h, bx, by, 4, &tmp);
            })?;
        }
    }

    Ok((out, width, height))
}

/// Iterate over 4x4 BC blocks of the top mip, calling `f(bx, by, block_bytes)`.
fn for_each_block(
    data: &[u8],
    w: usize,
    h: usize,
    block_size: usize,
    mut f: impl FnMut(usize, usize, &[u8]),
) -> Result<()> {
    let bw = w.div_ceil(4);
    let bh = h.div_ceil(4);
    let needed = bw * bh * block_size;
    if data.len() < needed {
        return Err(anyhow!(
            "DDS compressed data too small: {} < {}",
            data.len(),
            needed
        ));
    }
    for by in 0..bh {
        for bx in 0..bw {
            let offset = (by * bw + bx) * block_size;
            f(bx, by, &data[offset..offset + block_size]);
        }
    }
    Ok(())
}

/// Copy a decoded 4x4 block (`channels` bytes per pixel, 4-pixel row pitch)
/// into the destination image, clipping at image edges.
fn write_block(
    dst: &mut [u8],
    w: usize,
    h: usize,
    bx: usize,
    by: usize,
    channels: usize,
    block: &[u8],
) {
    for row in 0..4 {
        let y = by * 4 + row;
        if y >= h {
            break;
        }
        for col in 0..4 {
            let x = bx * 4 + col;
            if x >= w {
                break;
            }
            let src = (row * 4 + col) * channels;
            let dst_off = (y * w + x) * channels;
            dst[dst_off..dst_off + channels].copy_from_slice(&block[src..src + channels]);
        }
    }
}

/// A decoded greyscale-ish RGBA8 image (only the R channel is read).
pub type Rgba8Image = (Vec<u8>, u32, u32);

/// Default ORM texel: AO=255 (no occlusion), roughness≈0.78, metalness=0.
pub const DEFAULT_ORM: [u8; 4] = [255, 200, 0, 255];

/// Pack separate AO / roughness / metalness greyscale images into one RGBA8
/// linear texture: R=AO, G=roughness, B=metalness, A=255 (unused).
///
/// The output size is the largest present input (nearest-neighbour resampled);
/// missing channels use [`DEFAULT_ORM`] values. With no inputs a 1x1 default
/// texel is returned.
pub fn pack_orm(
    ao: Option<&Rgba8Image>,
    rough: Option<&Rgba8Image>,
    metal: Option<&Rgba8Image>,
) -> Rgba8Image {
    let (mut w, mut h) = (1u32, 1u32);
    for img in [ao, rough, metal].iter().copied().flatten() {
        if img.1 * img.2 > w * h {
            w = img.1;
            h = img.2;
        }
    }

    // Nearest-neighbour sample the R channel, or a constant default.
    let sample = |img: Option<&Rgba8Image>, default: u8, x: u32, y: u32| -> u8 {
        match img {
            Some((data, iw, ih)) if *iw > 0 && *ih > 0 => {
                let sx = (x * iw / w).min(iw - 1);
                let sy = (y * ih / h).min(ih - 1);
                data[((sy * iw + sx) * 4) as usize]
            }
            _ => default,
        }
    };

    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let off = ((y * w + x) * 4) as usize;
            out[off] = sample(ao, DEFAULT_ORM[0], x, y);
            out[off + 1] = sample(rough, DEFAULT_ORM[1], x, y);
            out[off + 2] = sample(metal, DEFAULT_ORM[2], x, y);
            out[off + 3] = 255;
        }
    }
    (out, w, h)
}

#[cfg(test)]
mod orm_tests {
    use super::*;

    fn solid(v: u8, w: u32, h: u32) -> Rgba8Image {
        let mut data = vec![0u8; (w * h * 4) as usize];
        for px in data.chunks_exact_mut(4) {
            px[0] = v;
            px[3] = 255;
        }
        (data, w, h)
    }

    #[test]
    fn pack_orm_defaults_when_missing() {
        let (data, w, h) = pack_orm(None, None, None);
        assert_eq!((w, h), (1, 1));
        assert_eq!(data, DEFAULT_ORM.to_vec());
    }

    #[test]
    fn pack_orm_resamples_to_largest() {
        let ao = solid(10, 1, 1);
        let rough = solid(20, 2, 2);
        let metal = solid(30, 4, 4);
        let (data, w, h) = pack_orm(Some(&ao), Some(&rough), Some(&metal));
        assert_eq!((w, h), (4, 4));
        for px in data.chunks_exact(4) {
            assert_eq!(px, &[10, 20, 30, 255]);
        }
    }

    #[test]
    fn pack_orm_partial_inputs_fill_defaults() {
        let rough = solid(99, 2, 2);
        let (data, w, h) = pack_orm(None, Some(&rough), None);
        assert_eq!((w, h), (2, 2));
        for px in data.chunks_exact(4) {
            assert_eq!(px, &[255, 99, 0, 255]);
        }
    }
}
