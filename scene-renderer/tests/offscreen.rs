//! Offscreen renderer tests: no window, event loop or runtime file paths
//! beyond loading the packaged shader set. GPU tests skip without an adapter
//! unless `NIF_VIEWER_REQUIRE_GPU` is set.

use scene_renderer::wgpu::TextureFormat;
use scene_renderer::*;
use scene_types::*;

fn shaders() -> ShaderSet {
    let dir = packaged_shader_dir();
    let manifest = std::fs::read_to_string(dir.join(MANIFEST_FILE)).unwrap();
    ShaderSet::load_with(&manifest, |f| {
        std::fs::read_to_string(dir.join(f)).map_err(|e| e.to_string())
    })
    .unwrap()
}

fn renderer(format: TextureFormat) -> Option<Renderer> {
    match futures::executor::block_on(GpuContext::new_offscreen(&ContextDesc::default())) {
        Ok(ctx) => Some(Renderer::new(&ctx, &shaders(), RendererDesc::new(format)).unwrap()),
        Err(RendererError::NoAdapter(_)) => {
            assert!(
                std::env::var_os("NIF_VIEWER_REQUIRE_GPU").is_none(),
                "no GPU adapter"
            );
            eprintln!("no wgpu adapter; skipping");
            None
        }
        Err(e) => panic!("{}", e),
    }
}

/// Camera-facing quad (normal +Z) filling the middle of the view.
fn quad_scene(base_color: [f32; 4]) -> SceneData {
    SceneData {
        meshes: vec![MeshData {
            name: "quad".into(),
            positions: vec![
                [-1.0, -1.0, 0.0],
                [1.0, -1.0, 0.0],
                [-1.0, 1.0, 0.0],
                [1.0, 1.0, 0.0],
            ],
            indices: vec![0, 1, 2, 2, 1, 3],
            uv0: Some(vec![[0.0, 1.0], [1.0, 1.0], [0.0, 0.0], [1.0, 0.0]]),
            ..MeshData::default()
        }],
        images: vec![],
        materials: vec![stock::BasicMaterial {
            base_color_factor: base_color,
            ..Default::default()
        }
        .into()],
        instances: vec![InstanceDesc::new(0, 0)],
    }
}

fn params() -> ViewParams {
    ViewParams {
        camera: Camera {
            eye: [0.0, 0.0, 3.0],
            ..Camera::default()
        },
        light: DirectionalLight {
            direction: [0.0, 0.0, -1.0],
            ..DirectionalLight::default()
        },
        clear_color: [0.0, 0.0, 0.0, 1.0],
    }
}

fn render(r: &Renderer, scene: Option<SceneId>, params: &ViewParams) -> CapturedImage {
    let view = r.create_view(ViewDesc::new(32, 32)).unwrap();
    futures::executor::block_on(r.render_to_image(&view, scene, params)).unwrap()
}

#[test]
fn renders_in_memory_scene_offscreen() {
    let Some(mut r) = renderer(TextureFormat::Rgba8UnormSrgb) else {
        return;
    };
    let (id, report) = r.upload_scene(&quad_scene([1.0, 0.0, 0.0, 1.0])).unwrap();
    assert!(report.diagnostics.is_empty());
    assert_eq!(report.stats.meshes, 1);
    let image = render(&r, Some(id), &params());
    let [red, green, blue, _] = image.pixel(16, 16);
    assert!(
        red > green + 100 && red > blue + 100,
        "centre {:?}",
        image.pixel(16, 16)
    );
    assert_eq!(image.pixel(0, 0), [0, 0, 0, 255]);
}

#[test]
fn vertex_colors_apply_only_when_enabled() {
    let Some(mut r) = renderer(TextureFormat::Rgba8UnormSrgb) else {
        return;
    };
    let mut scene = quad_scene([1.0; 4]);
    scene.meshes[0].colors = Some(vec![[0.0, 1.0, 0.0, 1.0]; 4]);
    let (off, _) = r.upload_scene(&scene).unwrap();
    scene.materials[0] = stock::BasicMaterial {
        use_vertex_colors: true,
        ..Default::default()
    }
    .into();
    let (on, _) = r.upload_scene(&scene).unwrap();
    let off = render(&r, Some(off), &params()).pixel(16, 16);
    let on = render(&r, Some(on), &params()).pixel(16, 16);
    assert!(
        off[0] > 150 && off[2] > 150,
        "white without vertex colours: {:?}",
        off
    );
    assert!(
        on[1] > on[0] + 100 && on[1] > on[2] + 100,
        "green with vertex colours: {:?}",
        on
    );
}

#[test]
fn linear_output_is_gamma_encoded_like_srgb_output() {
    let (Some(mut srgb), Some(mut linear)) = (
        renderer(TextureFormat::Rgba8UnormSrgb),
        renderer(TextureFormat::Rgba8Unorm),
    ) else {
        return;
    };
    let scene = quad_scene([0.5, 0.5, 0.5, 1.0]);
    let (a, _) = srgb.upload_scene(&scene).unwrap();
    let (b, _) = linear.upload_scene(&scene).unwrap();
    let a = render(&srgb, Some(a), &params()).pixel(16, 16);
    let b = render(&linear, Some(b), &params()).pixel(16, 16);
    // Manual pow(1/2.2) approximates the sRGB curve to within a few levels.
    assert!(
        a[0].abs_diff(b[0]) <= 6,
        "srgb {:?} vs manual gamma {:?}",
        a,
        b
    );
}

#[test]
fn mirrored_instance_keeps_front_lighting() {
    let Some(mut r) = renderer(TextureFormat::Rgba8UnormSrgb) else {
        return;
    };
    let mut scene = quad_scene([1.0; 4]);
    let (plain, _) = r.upload_scene(&scene).unwrap();
    scene.instances[0].transform[0][0] = -1.0;
    let (mirrored, _) = r.upload_scene(&scene).unwrap();
    let a = render(&r, Some(plain), &params()).pixel(16, 16);
    let b = render(&r, Some(mirrored), &params()).pixel(16, 16);
    for c in 0..3 {
        assert!(a[c].abs_diff(b[c]) <= 2, "plain {:?} mirrored {:?}", a, b);
    }
}

#[test]
fn textures_bind_with_declared_color_space() {
    let Some(mut r) = renderer(TextureFormat::Rgba8UnormSrgb) else {
        return;
    };
    let mut scene = quad_scene([1.0; 4]);
    scene.images.push(ImageData::rgba8(
        "blue",
        1,
        1,
        ColorSpace::Srgb,
        vec![0, 0, 255, 255],
    ));
    scene.materials[0].textures = vec![Some(TextureRef::new(0))];
    let (id, report) = r.upload_scene(&scene).unwrap();
    assert_eq!(report.stats.images, 1);
    let px = render(&r, Some(id), &params()).pixel(16, 16);
    assert!(px[2] > px[0] + 100 && px[2] > px[1] + 100, "{:?}", px);
}

#[test]
fn handles_are_generational_and_scoped_to_renderer() {
    let (Some(mut r), Some(other)) = (
        renderer(TextureFormat::Rgba8UnormSrgb),
        renderer(TextureFormat::Rgba8UnormSrgb),
    ) else {
        return;
    };
    let (first, _) = r.upload_scene(&quad_scene([1.0; 4])).unwrap();
    let (second, _) = r.upload_scene(&quad_scene([1.0; 4])).unwrap();
    assert_eq!(r.live_scene_count(), 2);
    assert!(r.release_scene(first));
    assert!(!r.release_scene(first));
    assert_eq!(r.live_scene_count(), 1);
    // Slot reuse must not revive the stale handle.
    let (third, _) = r.upload_scene(&quad_scene([1.0; 4])).unwrap();
    assert_ne!(first, third);
    assert!(matches!(
        r.scene_stats(first),
        Err(RendererError::InvalidHandle(_))
    ));
    assert!(r.scene_stats(second).is_ok());
    assert!(matches!(
        other.scene_stats(second),
        Err(RendererError::InvalidHandle(_))
    ));
}

#[test]
fn failed_upload_leaves_existing_scenes_untouched() {
    let Some(mut r) = renderer(TextureFormat::Rgba8UnormSrgb) else {
        return;
    };
    let (good, _) = r.upload_scene(&quad_scene([1.0; 4])).unwrap();
    let mut bad = quad_scene([1.0; 4]);
    bad.meshes[0].indices.push(9);
    bad.meshes[0].indices.extend([0, 1]);
    assert!(matches!(r.upload_scene(&bad), Err(RendererError::Scene(_))));
    assert_eq!(r.live_scene_count(), 1);
    assert!(r.scene_stats(good).is_ok());
}

#[test]
fn unsupported_alpha_is_reported_or_rejected_when_strict() {
    let Some(mut r) = renderer(TextureFormat::Rgba8UnormSrgb) else {
        return;
    };
    let mut scene = quad_scene([1.0; 4]);
    scene.materials[0].alpha_mode = AlphaMode::Blend;
    let (_, report) = r.upload_scene(&scene).unwrap();
    assert_eq!(report.diagnostics.len(), 1);

    let ctx = r.context().clone();
    let mut strict = Renderer::new(
        &ctx,
        &shaders(),
        RendererDesc {
            strict: true,
            ..RendererDesc::new(TextureFormat::Rgba8UnormSrgb)
        },
    )
    .unwrap();
    assert!(matches!(
        strict.upload_scene(&scene),
        Err(RendererError::Unsupported(_))
    ));
}

#[test]
fn instance_updates_move_geometry() {
    let Some(mut r) = renderer(TextureFormat::Rgba8UnormSrgb) else {
        return;
    };
    let scene = quad_scene([1.0; 4]);
    let (id, _) = r.upload_scene(&scene).unwrap();
    let mut moved = scene.instances.clone();
    moved[0].transform[3] = [100.0, 0.0, 0.0, 1.0];
    r.update_instances(id, &moved).unwrap();
    assert_eq!(
        render(&r, Some(id), &params()).pixel(16, 16),
        [0, 0, 0, 255]
    );
    moved[0].visible = true;
    moved[0].transform[2] = [0.0; 4];
    assert!(r.update_instances(id, &moved).is_err());
}

#[test]
fn capture_observes_the_frame_it_was_encoded_with() {
    let Some(r) = renderer(TextureFormat::Bgra8UnormSrgb) else {
        return;
    };
    let view = r.create_view(ViewDesc::new(8, 4)).unwrap();
    let output = r.create_output_texture(8, 4);
    let target = output.create_view(&Default::default());
    let mut results = Vec::new();
    for (i, colour) in [[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0]]
        .iter()
        .enumerate()
    {
        let mut encoder = r.context().create_encoder("capture test");
        let p = ViewParams {
            clear_color: *colour,
            ..params()
        };
        let receipt = r.encode(&mut encoder, &view, None, &p, &target).unwrap();
        let capture = r
            .encode_capture(
                &mut encoder,
                &receipt,
                CaptureSource::Texture(&output),
                &format!("f{}", i),
            )
            .unwrap();
        r.context().submit(encoder);
        let (tx, rx) = std::sync::mpsc::channel();
        capture.map(move |img| tx.send(img).unwrap());
        r.context()
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        results.push((receipt.frame, rx.recv().unwrap().unwrap()));
    }
    let (frame0, red) = &results[0];
    let (frame1, green) = &results[1];
    assert_eq!(red.frame, *frame0);
    assert_eq!(green.frame, *frame1);
    // BGRA output is swizzled to RGBA and padding rows are stripped.
    assert_eq!(red.rgba.len(), 8 * 4 * 4);
    assert_eq!(red.pixel(7, 3), [255, 0, 0, 255]);
    assert_eq!(green.pixel(0, 0), [0, 255, 0, 255]);

    // Scene-colour (float) captures convert to 8-bit sRGB.
    let mut encoder = r.context().create_encoder("scene colour");
    let p = ViewParams {
        clear_color: [0.5, 0.0, 0.0, 1.0],
        ..params()
    };
    let receipt = r.encode(&mut encoder, &view, None, &p, &target).unwrap();
    let capture = r
        .encode_capture(
            &mut encoder,
            &receipt,
            CaptureSource::SceneColor(&view),
            "scene",
        )
        .unwrap();
    r.context().submit(encoder);
    let (tx, rx) = std::sync::mpsc::channel();
    capture.map(move |img| tx.send(img).unwrap());
    r.context()
        .device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    let scene = rx.recv().unwrap().unwrap();
    assert!(
        scene.pixel(0, 0)[0].abs_diff(188) <= 1,
        "{:?}",
        scene.pixel(0, 0)
    );
}

#[test]
fn zero_sized_views_are_suspended() {
    let Some(r) = renderer(TextureFormat::Rgba8UnormSrgb) else {
        return;
    };
    let mut view = r.create_view(ViewDesc::new(0, 10)).unwrap();
    assert!(view.is_suspended());
    let output = r.create_output_texture(4, 4);
    let mut encoder = r.context().create_encoder("suspended");
    let target = output.create_view(&Default::default());
    assert!(matches!(
        r.encode(&mut encoder, &view, None, &params(), &target),
        Err(RendererError::Suspended)
    ));
    r.resize_view(&mut view, ViewDesc::new(4, 4)).unwrap();
    assert!(!view.is_suspended());
}

#[test]
fn shader_abi_mismatch_and_missing_stages_are_rejected() {
    let manifest = format!(
        r#"{{"abi_version":{},"files":{{}}}}"#,
        SHADER_ABI_VERSION + 1
    );
    assert!(ShaderManifest::parse(&manifest).is_err());

    // A material-only set cannot construct a renderer.
    let materials_only = format!(
        r#"{{"abi_version":{},"materials":{{}}}}"#,
        SHADER_ABI_VERSION
    );
    let set = ShaderSet::load_with(&materials_only, |_| Ok(String::new())).unwrap();
    if let Ok(ctx) = futures::executor::block_on(GpuContext::new_offscreen(&ContextDesc::default()))
    {
        let err = Renderer::new(&ctx, &set, RendererDesc::new(TextureFormat::Rgba8UnormSrgb)).err();
        assert!(matches!(err, Some(RendererError::Shader(_))));
    }
}

const UNLIT: &str = r#"
struct Unlit { colour: vec4<f32> }
@group(0) @binding(0)
var<uniform> unlit: Unlit;

@vertex
fn vertex_main(model: VertexInput, instance: InstanceInput) -> VertexOutput {
    return standard_vertex(model, instance);
}

@fragment
fn fragment_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return unlit.colour;
}
"#;

fn unlit_type() -> MaterialTypeDesc {
    MaterialTypeDesc {
        key: "test-unlit".into(),
        source: UNLIT.into(),
        textures: vec![],
        uniform_size: 16,
        default_uniforms: vec![0; 16],
    }
}

fn unlit(colour: [f32; 4]) -> MaterialDesc {
    MaterialDesc {
        name: "unlit".into(),
        material_type: "test-unlit".into(),
        uniforms: colour.iter().flat_map(|v| v.to_le_bytes()).collect(),
        ..MaterialDesc::default()
    }
}

#[test]
fn custom_material_type_renders_its_own_shader() {
    let Some(mut r) = renderer(TextureFormat::Rgba8UnormSrgb) else {
        return;
    };
    assert_eq!(r.material_types().collect::<Vec<_>>(), [BASIC_MATERIAL]);
    r.register_material_type(&unlit_type()).unwrap();
    let mut scene = quad_scene([1.0; 4]);
    scene.materials[0] = unlit([0.0, 0.0, 1.0, 1.0]);
    let (id, _) = r.upload_scene(&scene).unwrap();
    // Linear 1.0 blue, no lighting, sRGB-encoded output.
    assert_eq!(
        render(&r, Some(id), &params()).pixel(16, 16),
        [0, 0, 255, 255]
    );
}

#[test]
fn scenes_mix_material_types() {
    let Some(mut r) = renderer(TextureFormat::Rgba8UnormSrgb) else {
        return;
    };
    r.register_material_type(&unlit_type()).unwrap();
    let mut scene = quad_scene([1.0, 0.0, 0.0, 1.0]);
    scene.materials.push(unlit([0.0, 1.0, 0.0, 1.0]));
    // Interleave types so the sorted draw order differs from instance order.
    let at = |x: f32, material: usize| InstanceDesc {
        transform: [
            [0.25, 0.0, 0.0, 0.0],
            [0.0, 0.25, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [x, 0.0, 0.0, 1.0],
        ],
        ..InstanceDesc::new(0, material)
    };
    scene.instances = vec![at(-0.6, 0), at(0.0, 1), at(0.6, 0)];
    let (id, report) = r.upload_scene(&scene).unwrap();
    assert_eq!(report.stats.materials, 2);
    let image = render(&r, Some(id), &params());
    let [lr, lg, _, _] = image.pixel(9, 16);
    let [rr, rg, _, _] = image.pixel(22, 16);
    assert!(
        lr > lg + 100 && rr > rg + 100,
        "basic quads: {:?} {:?}",
        image.pixel(9, 16),
        image.pixel(22, 16)
    );
    assert_eq!(image.pixel(16, 16), [0, 255, 0, 255]);
}

#[test]
fn material_type_errors_are_reported() {
    let Some(mut r) = renderer(TextureFormat::Rgba8UnormSrgb) else {
        return;
    };
    let mut scene = quad_scene([1.0; 4]);
    scene.materials[0] = unlit([1.0; 4]);
    assert!(matches!(
        r.upload_scene(&scene),
        Err(RendererError::Scene(SceneError::UnknownMaterialType { .. }))
    ));

    r.register_material_type(&unlit_type()).unwrap();
    assert!(matches!(
        r.register_material_type(&unlit_type()),
        Err(RendererError::Shader(_))
    ));
    scene.materials[0].uniforms.truncate(8);
    assert!(matches!(
        r.upload_scene(&scene),
        Err(RendererError::Scene(SceneError::MaterialBinding { .. }))
    ));

    let broken = MaterialTypeDesc {
        key: "broken".into(),
        source: "fn fragment_main( {".into(),
        ..unlit_type()
    };
    let err = r.register_material_type(&broken).unwrap_err();
    assert!(err.to_string().contains("broken"), "{}", err);
    assert!(!r.material_types().any(|k| k == "broken"));
}
