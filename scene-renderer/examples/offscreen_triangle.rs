//! Minimal external consumer: render an in-memory triangle offscreen and
//! write `triangle.ppm`. No window, event loop or NIF code is involved.
//!
//! `cargo run -p scene-renderer --example offscreen_triangle`

use scene_renderer::*;
use scene_types::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    futures::executor::block_on(async {
        let ctx = GpuContext::new_offscreen(&ContextDesc::default()).await?;
        let dir = packaged_shader_dir();
        let manifest = std::fs::read_to_string(dir.join(MANIFEST_FILE))?;
        let shaders = ShaderSet::load_with(&manifest, |f| {
            std::fs::read_to_string(dir.join(f)).map_err(|e| e.to_string())
        })?;
        let mut renderer = Renderer::new(
            &ctx,
            &shaders,
            RendererDesc::new(wgpu::TextureFormat::Rgba8UnormSrgb),
        )?;

        let scene = SceneData {
            meshes: vec![MeshData {
                name: "triangle".into(),
                positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]],
                indices: vec![0, 1, 2],
                colors: Some(vec![
                    [1.0, 0.0, 0.0, 1.0],
                    [0.0, 1.0, 0.0, 1.0],
                    [0.0, 0.0, 1.0, 1.0],
                ]),
                ..MeshData::default()
            }],
            materials: vec![stock::BasicMaterial {
                use_vertex_colors: true,
                ..Default::default()
            }
            .into()],
            instances: vec![InstanceDesc::new(0, 0)],
            images: vec![],
        };
        let (id, _) = renderer.upload_scene(&scene)?;
        let view = renderer.create_view(ViewDesc::new(256, 256))?;
        let params = ViewParams {
            camera: Camera {
                eye: [0.0, 0.0, 3.0],
                ..Camera::default()
            },
            light: DirectionalLight {
                direction: [0.0, 0.0, -1.0],
                ..DirectionalLight::default()
            },
            ..ViewParams::default()
        };
        let image = renderer.render_to_image(&view, Some(id), &params).await?;

        let mut ppm = format!("P6 {} {} 255\n", image.width, image.height).into_bytes();
        for px in image.rgba.as_chunks::<4>().0 {
            ppm.extend_from_slice(&px[..3]);
        }
        std::fs::write("triangle.ppm", ppm)?;
        println!("wrote triangle.ppm");
        Ok(())
    })
}
