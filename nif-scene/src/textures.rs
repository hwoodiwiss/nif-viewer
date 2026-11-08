//! Game texture decoding: DDS (BC1-5, BC7, uncompressed) to RGBA8 and ORM
//! packing. The output is plain RGBA8 for [`scene_types::ImageData`]; the
//! renderer never sees container formats.

/// Tightly packed RGBA8 pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba8Image {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("failed to parse DDS: {0}")]
    Parse(String),
    #[error("DDS has zero dimensions")]
    ZeroSize,
    #[error("unsupported DDS format: {0}")]
    Unsupported(String),
    #[error("DDS data too small: {actual} < {needed}")]
    Truncated { actual: usize, needed: usize },
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

/// Decode the top mip level of a DDS file (BC1-5, BC7, RGBA8/BGRA8) to RGBA8.
///
/// With `reconstruct_normal_z`, two-channel BC5 data is treated as a
/// tangent-space normal map and Z is rebuilt as `sqrt(1 - x² - y²)`.
pub fn decode_dds_rgba8(
    bytes: &[u8],
    reconstruct_normal_z: bool,
) -> Result<Rgba8Image, ImageError> {
    use ddsfile::{D3DFormat, Dds, DxgiFormat};

    let dds = Dds::read(bytes).map_err(|e| ImageError::Parse(e.to_string()))?;
    let width = dds.get_width();
    let height = dds.get_height();
    if width == 0 || height == 0 {
        return Err(ImageError::ZeroSize);
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
            other => return Err(ImageError::Unsupported(format!("DXGI {:?}", other))),
        }
    } else if let Some(fmt) = dds.get_d3d_format() {
        match fmt {
            D3DFormat::DXT1 => DdsCodec::Bc1,
            D3DFormat::DXT2 | D3DFormat::DXT3 => DdsCodec::Bc2,
            D3DFormat::DXT4 | D3DFormat::DXT5 => DdsCodec::Bc3,
            D3DFormat::A8B8G8R8 => DdsCodec::Rgba8,
            D3DFormat::A8R8G8B8 => DdsCodec::Bgra8,
            other => return Err(ImageError::Unsupported(format!("D3D {:?}", other))),
        }
    } else if let Some(fc) = &dds.header.spf.fourcc {
        // Legacy FourCC codes that ddsfile's D3DFormat enum does not map.
        match fc.0 {
            x if x == fourcc(b"ATI1") || x == fourcc(b"BC4U") => DdsCodec::Bc4,
            x if x == fourcc(b"BC4S") => DdsCodec::Bc4Snorm,
            x if x == fourcc(b"ATI2") || x == fourcc(b"BC5U") => DdsCodec::Bc5,
            x if x == fourcc(b"BC5S") => DdsCodec::Bc5Snorm,
            other => return Err(ImageError::Unsupported(format!("FourCC {:#010x}", other))),
        }
    } else {
        return Err(ImageError::Unsupported(
            "no recognizable pixel format".into(),
        ));
    };

    let data = dds
        .get_data(0)
        .map_err(|e| ImageError::Parse(e.to_string()))?;

    let (w, h) = (width as usize, height as usize);
    let mut out = vec![0u8; w * h * 4];

    match codec {
        DdsCodec::Rgba8 | DdsCodec::Bgra8 => {
            let needed = w * h * 4;
            if data.len() < needed {
                return Err(ImageError::Truncated {
                    actual: data.len(),
                    needed,
                });
            }
            out.copy_from_slice(&data[..needed]);
            if matches!(codec, DdsCodec::Bgra8) {
                for px in out.as_chunks_mut::<4>().0 {
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
            for (i, px) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
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
            for (i, px) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let r = rg[i * 2];
                let g = rg[i * 2 + 1];
                px[0] = r;
                px[1] = g;
                px[2] = if reconstruct_normal_z {
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

    Ok(Rgba8Image {
        pixels: out,
        width,
        height,
    })
}

/// Iterate over 4x4 BC blocks of the top mip, calling `f(bx, by, block_bytes)`.
fn for_each_block(
    data: &[u8],
    w: usize,
    h: usize,
    block_size: usize,
    mut f: impl FnMut(usize, usize, &[u8]),
) -> Result<(), ImageError> {
    let bw = w.div_ceil(4);
    let bh = h.div_ceil(4);
    let needed = bw * bh * block_size;
    if data.len() < needed {
        return Err(ImageError::Truncated {
            actual: data.len(),
            needed,
        });
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

/// Default ORM texel: AO=255 (no occlusion), roughness≈0.78, metalness=0.
pub const DEFAULT_ORM: [u8; 4] = [255, 200, 0, 255];

/// Pack separate AO / roughness / metalness greyscale images (R channel read)
/// into one RGBA8 linear texture: R=AO, G=roughness, B=metalness, A=255.
///
/// The output size is the largest present input (nearest-neighbour resampled);
/// missing channels use [`DEFAULT_ORM`]. With no inputs a 1x1 default texel
/// is returned.
pub fn pack_orm(
    ao: Option<&Rgba8Image>,
    rough: Option<&Rgba8Image>,
    metal: Option<&Rgba8Image>,
) -> Rgba8Image {
    let (mut w, mut h) = (1u32, 1u32);
    for img in [ao, rough, metal].iter().copied().flatten() {
        if img.width * img.height > w * h {
            w = img.width;
            h = img.height;
        }
    }

    let sample = |img: Option<&Rgba8Image>, default: u8, x: u32, y: u32| -> u8 {
        match img {
            Some(img) if img.width > 0 && img.height > 0 => {
                let sx = (x * img.width / w).min(img.width - 1);
                let sy = (y * img.height / h).min(img.height - 1);
                img.pixels[((sy * img.width + sx) * 4) as usize]
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
    Rgba8Image {
        pixels: out,
        width: w,
        height: h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(v: u8, w: u32, h: u32) -> Rgba8Image {
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        for px in pixels.as_chunks_mut::<4>().0 {
            px[0] = v;
            px[3] = 255;
        }
        Rgba8Image {
            pixels,
            width: w,
            height: h,
        }
    }

    #[test]
    fn pack_orm_defaults_when_missing() {
        let img = pack_orm(None, None, None);
        assert_eq!((img.width, img.height), (1, 1));
        assert_eq!(img.pixels, DEFAULT_ORM.to_vec());
    }

    #[test]
    fn pack_orm_resamples_to_largest() {
        let img = pack_orm(
            Some(&solid(10, 1, 1)),
            Some(&solid(20, 2, 2)),
            Some(&solid(30, 4, 4)),
        );
        assert_eq!((img.width, img.height), (4, 4));
        for px in img.pixels.as_chunks::<4>().0 {
            assert_eq!(px, &[10, 20, 30, 255]);
        }
    }

    #[test]
    fn pack_orm_partial_inputs_fill_defaults() {
        let img = pack_orm(None, Some(&solid(99, 2, 2)), None);
        assert_eq!((img.width, img.height), (2, 2));
        for px in img.pixels.as_chunks::<4>().0 {
            assert_eq!(px, &[255, 99, 0, 255]);
        }
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode_dds_rgba8(b"nope", false).is_err());
    }
}
