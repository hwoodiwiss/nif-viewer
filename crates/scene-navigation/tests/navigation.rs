use cgmath::{InnerSpace, Point3, Vector3};
use scene_navigation::{
    Action, Bounds, Camera, GamepadSample, InputFrame, InputState, Mode, NavigationSettings, Rig,
};
use winit::keyboard::KeyCode;

fn camera() -> Camera {
    Camera {
        eye: Point3::new(0.0, 0.0, 2.0),
        target: Point3::new(0.0, 0.0, 0.0),
        up: Vector3::unit_y(),
        aspect: 1.0,
        fov_y: 45.0,
        z_near: 0.1,
        z_far: 1000.0,
    }
}

#[test]
fn translation_is_time_based_and_passes_the_old_target() {
    for hz in [30, 60, 144] {
        let mut camera = camera();
        let mut rig = Rig::new(&camera, 12.0);
        for _ in 0..hz {
            rig.update(
                &mut camera,
                &InputFrame {
                    movement: [0.0, 0.0, 1.0],
                    ..Default::default()
                },
                1.0 / hz as f32,
            );
        }
        assert!((camera.eye.z + 10.0).abs() < 0.0001);
        assert!((camera.target.z + 12.0).abs() < 0.0001);
    }
}

#[test]
fn diagonal_motion_does_not_exceed_speed() {
    let mut camera = camera();
    let start = camera.eye;
    let mut rig = Rig::new(&camera, 10.0);
    rig.update(
        &mut camera,
        &InputFrame {
            movement: [1.0, 1.0, 1.0],
            ..Default::default()
        },
        0.05,
    );
    assert!(((camera.eye - start).magnitude() - 0.5).abs() < 1e-5);
}

#[test]
fn mouse_displacement_is_not_multiplied_by_time() {
    let mut a = camera();
    let mut b = camera();
    let mut ra = Rig::new(&a, 1.0);
    let mut rb = Rig::new(&b, 1.0);
    ra.update(
        &mut a,
        &InputFrame {
            mouse: [100.0, 50.0],
            ..Default::default()
        },
        0.03,
    );
    for _ in 0..10 {
        rb.update(
            &mut b,
            &InputFrame {
                mouse: [10.0, 5.0],
                ..Default::default()
            },
            0.001,
        );
    }
    assert!((a.target - b.target).magnitude() < 1e-5);
}

#[test]
fn aliases_and_opposing_keys_are_independent() {
    let mut input = InputState::default();
    input.active = true;
    input.key(KeyCode::KeyE, true, false, false);
    input.key(KeyCode::Space, true, false, false);
    input.key(KeyCode::KeyE, false, false, false);
    assert_eq!(
        input
            .frame(&[], &NavigationSettings::default(), true)
            .movement[1],
        1.0
    );
    input.key(KeyCode::KeyQ, true, false, false);
    assert_eq!(
        input
            .frame(&[], &NavigationSettings::default(), true)
            .movement[1],
        0.0
    );
}

#[test]
fn blur_clears_keys_deltas_and_pending_commands() {
    let mut input = InputState::default();
    input.active = true;
    input.key(KeyCode::KeyW, true, false, false);
    input.key(KeyCode::KeyO, true, false, false);
    input.mouse(50.0, 50.0);
    input.clear();
    input.active = true;
    let frame = input.frame(&[], &NavigationSettings::default(), true);
    assert_eq!(frame.movement, [0.0; 3]);
    assert_eq!(frame.mouse, [0.0; 2]);
    assert!(frame.commands.is_empty());
}

#[test]
fn rapid_press_release_and_repeat_generate_one_command() {
    let mut input = InputState::default();
    input.active = true;
    input.key(KeyCode::KeyO, true, false, false);
    input.key(KeyCode::KeyO, true, true, false);
    input.key(KeyCode::KeyO, false, false, false);
    assert_eq!(
        input
            .frame(&[], &NavigationSettings::default(), true)
            .commands,
        [Action::ToggleMode]
    );
    assert!(input
        .frame(&[], &NavigationSettings::default(), true)
        .commands
        .is_empty());
}

#[test]
fn gamepad_requires_neutral_then_activation_and_disconnect_stops_motion() {
    let settings = NavigationSettings::default();
    let mut input = InputState::default();
    let mut pad = GamepadSample::default();
    pad.buttons[0] = true;
    input.frame(&[pad.clone()], &settings, true);
    assert!(!input.active);
    pad.buttons[0] = false;
    input.frame(&[pad.clone()], &settings, true);
    pad.buttons[0] = true;
    input.frame(&[pad.clone()], &settings, true);
    assert!(input.active);
    pad.left = [0.0, 1.0];
    pad.triggers = [0.5, 0.5];
    let frame = input.frame(&[pad], &settings, true);
    assert_eq!(frame.movement, [0.0, 0.0, 1.0]);
    let frame = input.frame(&[], &settings, true);
    assert_eq!(frame.movement, [0.0; 3]);
    assert!(!input.active);
}

#[test]
fn idle_sticks_do_not_drift_and_partial_input_is_preserved() {
    let settings = NavigationSettings::default();
    let mut input = InputState::default();
    let mut pad = GamepadSample::default();
    input.frame(&[pad.clone()], &settings, true);
    pad.buttons[0] = true;
    input.frame(&[pad.clone()], &settings, true);
    pad.left = [0.05, -0.05];
    assert_eq!(
        input.frame(&[pad.clone()], &settings, true).movement,
        [0.0; 3]
    );
    pad.left = [0.575, 0.0];
    assert!((input.frame(&[pad], &settings, true).movement[0] - 0.5).abs() < 1e-6);
}

#[test]
fn orbit_switch_preserves_pose_and_framing_uses_aspect() {
    let mut camera = camera();
    let mut rig = Rig::new(&camera, 12.0);
    let eye = camera.eye;
    rig.set_mode(Mode::Orbit, &camera);
    rig.update(&mut camera, &InputFrame::default(), 0.01);
    assert!((camera.eye - eye).magnitude() < 1e-5);
    let bounds = Bounds::new([10.0, 2.0, -5.0], 2.0).unwrap();
    rig.set_bounds(bounds, &mut camera, true);
    let wide = (camera.eye - bounds.center).magnitude();
    camera.aspect = 0.2;
    rig.frame(&mut camera);
    assert!((camera.eye - bounds.center).magnitude() > wide);
    assert_eq!(camera.target, bounds.center);
}

#[test]
fn invalid_inputs_and_long_pauses_remain_finite_and_bounded() {
    let mut camera = camera();
    let mut rig = Rig::new(&camera, 10.0);
    rig.update(
        &mut camera,
        &InputFrame {
            movement: [f32::NAN, 0.0, 1.0],
            mouse: [f32::INFINITY, 1e10],
            ..Default::default()
        },
        100.0,
    );
    assert!((camera.eye - Point3::new(0.0, 0.0, 2.0)).magnitude() <= 0.501);
    let matrix: [[f32; 4]; 4] = camera.build_view_projection_matrix().into();
    assert!(matrix.iter().flatten().all(|v| v.is_finite()));
    assert!(NavigationSettings {
        speed: f32::NAN,
        ..Default::default()
    }
    .validate()
    .is_err());
}

#[test]
fn rate_look_is_time_based_and_precision_wins() {
    let mut targets = vec![];
    for hz in [30, 60, 144] {
        let mut camera = camera();
        let mut rig = Rig::new(&camera, 10.0);
        for _ in 0..hz {
            rig.update(
                &mut camera,
                &InputFrame {
                    look: [1.0, 0.0],
                    ..Default::default()
                },
                1.0 / hz as f32,
            );
        }
        targets.push(camera.target);
    }
    assert!((targets[0] - targets[2]).magnitude() < 1e-5);
    let mut camera = camera();
    let mut rig = Rig::new(&camera, 10.0);
    rig.update(
        &mut camera,
        &InputFrame {
            movement: [0.0, 0.0, 1.0],
            boost: true,
            precision: true,
            ..Default::default()
        },
        0.05,
    );
    assert!((camera.eye.z - 1.9).abs() < 1e-5);
}

#[test]
fn explicit_speed_survives_scene_load_and_reset_is_recoverable() {
    let mut camera = camera();
    let mut rig = Rig::new(&camera, 10.0);
    rig.configure(
        NavigationSettings {
            speed: 7.0,
            automatic_speed: false,
            ..Default::default()
        },
        &camera,
    )
    .unwrap();
    rig.set_bounds(
        Bounds::new([100.0, -20.0, 50.0], 1000.0).unwrap(),
        &mut camera,
        true,
    );
    let framed = camera.eye;
    assert_eq!(rig.settings.speed, 7.0);
    rig.update(
        &mut camera,
        &InputFrame {
            movement: [1.0, 1.0, 1.0],
            ..Default::default()
        },
        0.05,
    );
    rig.update(
        &mut camera,
        &InputFrame {
            commands: vec![Action::Reset],
            ..Default::default()
        },
        0.05,
    );
    assert_eq!(camera.eye, framed);
}

#[test]
fn second_controller_cannot_steal_and_release_requires_rearming() {
    let settings = NavigationSettings::default();
    let mut input = InputState::default();
    let mut first = GamepadSample::default();
    let mut second = GamepadSample {
        id: 1,
        ..Default::default()
    };
    input.frame(&[first.clone(), second.clone()], &settings, true);
    first.buttons[0] = true;
    input.frame(&[first.clone(), second.clone()], &settings, true);
    second.buttons[0] = true;
    second.left = [1.0, 0.0];
    assert_eq!(
        input
            .frame(&[first.clone(), second.clone()], &settings, true)
            .movement,
        [0.0; 3]
    );
    first.buttons[1] = true;
    input.frame(&[first.clone(), second.clone()], &settings, true);
    assert!(!input.active);
    input.frame(&[first, second], &settings, true);
    assert!(!input.active);
}
