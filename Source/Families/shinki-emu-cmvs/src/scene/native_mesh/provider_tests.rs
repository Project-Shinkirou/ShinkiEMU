use super::*;
use crate::scene::parent_load_tests::{command, fixture, write_image};

fn gpu_scene() -> CmvsScene {
    let mut scene = CmvsScene::new(crate::scene::tests::gpu_instance()).unwrap();
    scene.clear(32, 32).unwrap();
    scene
}
fn stage(vm: &mut CmvsPs2aVmState) {
    vm.native_stage_size = [32, 32];
    vm.native_render_size = [32, 32];
}
fn solid_quad(id: &str, depth: f32, color: [f32; 4], depth_mode: MeshDepth2D) -> SceneCommand {
    SceneCommand::ProjectiveMesh2D {
        id: id.into(),
        vertices: Arc::from(
            [[0.0, 0.0], [32.0, 0.0], [0.0, 32.0], [32.0, 32.0]].map(|xy| ProjectiveMeshVertex2D {
                position: [xy[0], xy[1], depth, 1.0],
                uv: [0.5; 2],
                premultiplied_rgba: color,
            }),
        ),
        indices: Arc::from(crate::vm::native_mesh::MESH_TRIANGLES),
        material: MeshMaterial2D::Solid,
        texture_id: None,
        texture_filter: TextureFilter2D::Nearest,
        opacity: 1.0,
        blend: BlendMode::Opaque,
        depth: depth_mode,
    }
}
fn center(frame: &TextureFrame) -> &[u8] {
    &frame.rgba8[(16 * 32 + 16) * 4..(16 * 32 + 16) * 4 + 3]
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_repeated_resource_request_reloads_only_the_selected_live_provider() {
    let (root, archive, mut vm) = fixture();
    stage(&mut vm);
    for name in crate::CmvsNativeSceneKind54::RESOURCES {
        write_image(
            root.path(),
            std::str::from_utf8(name).unwrap(),
            16,
            16,
            [255, 0, 0, 255],
        );
    }
    write_image(root.path(), "try008z2.pb3", 16, 16, [0, 0, 255, 255]);
    command(&mut vm, 660, &[0]);
    for _ in 0..25 {
        crate::vm::native_scene_worker::tick(&mut vm, 200).unwrap();
    }
    let mut scene = gpu_scene();
    let commands = scene
        .native_mesh_commands(&vm, &archive, (32, 32), [517, 0, 0, 0])
        .unwrap();
    scene.draw(32, 32, commands).unwrap();
    // These files change on disk, but request50 only reloads the sixth root.
    for name in &crate::CmvsNativeSceneKind54::RESOURCES[..5] {
        write_image(
            root.path(),
            std::str::from_utf8(name).unwrap(),
            16,
            16,
            [0, 255, 0, 255],
        );
    }
    command(&mut vm, 687, &[0, 50, 0, 0]);
    let commands = scene
        .native_mesh_commands(&vm, &archive, (32, 32), [517, 0, 0, 0])
        .unwrap();
    let changed = scene.draw(32, 32, commands).unwrap();
    assert!(
        changed
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[1] == 0)
    );
    assert!(
        changed
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[2] > 32)
    );
    write_image(root.path(), "try008z2.pb3", 16, 16, [0, 255, 0, 255]);
    command(&mut vm, 687, &[0, 50, 0, 0]);
    let commands = scene
        .native_mesh_commands(&vm, &archive, (32, 32), [517, 0, 0, 0])
        .unwrap();
    let repeated = scene.draw(32, 32, commands).unwrap();
    assert!(
        repeated
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[1] > 32)
    );
    assert!(
        repeated
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[2] == 0)
    );
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_zero_alpha_mesh_does_not_occlude_later_geometry() {
    let (root, archive, mut vm) = fixture();
    stage(&mut vm);
    for (index, name) in crate::CmvsNativeSceneKind57::RESOURCES.iter().enumerate() {
        write_image(
            root.path(),
            std::str::from_utf8(name).unwrap(),
            16,
            16,
            if index == 0 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 0, 0]
            },
        );
    }
    command(&mut vm, 663, &[0]);
    let mut scene = gpu_scene();
    let mut commands = scene
        .native_mesh_commands(&vm, &archive, (32, 32), [200, 0, 0, 0])
        .unwrap();
    // Between root0(z32) and the invisible root1(z31.9).
    commands.push(solid_quad(
        "later-blue",
        0.99787,
        [0.0, 0.0, 1.0, 1.0],
        MeshDepth2D::ReadWriteLess,
    ));
    let frame = scene.draw(32, 32, commands).unwrap();
    assert_eq!(center(&frame), [0, 0, 255]);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_opaque_mesh_keeps_source_rgb_even_when_texture_alpha_is_zero() {
    let (root, archive, mut vm) = fixture();
    stage(&mut vm);
    for (index, name) in crate::CmvsNativeSceneKind57::RESOURCES.iter().enumerate() {
        write_image(
            root.path(),
            std::str::from_utf8(name).unwrap(),
            16,
            16,
            if index == 0 {
                [255, 0, 0, 0]
            } else {
                [0, 0, 0, 0]
            },
        );
    }
    command(&mut vm, 663, &[0]);
    let mut scene = gpu_scene();
    let commands = scene
        .native_mesh_commands(&vm, &archive, (32, 32), [200, 0, 0, 0])
        .unwrap();
    let frame = scene.draw(32, 32, commands).unwrap();
    assert_eq!(center(&frame), [255, 0, 0]);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_additive_mesh_retains_destination_red_instead_of_alpha_dimming_it() {
    let (root, archive, mut vm) = fixture();
    stage(&mut vm);
    write_image(root.path(), "blue.pb3", 16, 16, [0, 0, 255, 128]);
    let mut rng = 1;
    let mut snow = crate::CmvsNativeSnow::new(
        b"blue.pb3".to_vec(),
        [
            1.0_f32.to_bits(),
            1.0_f32.to_bits(),
            1000.0_f32.to_bits(),
            1.0_f32.to_bits(),
            1,
            0,
        ],
        &mut rng,
    )
    .unwrap();
    let mesh = &mut snow.particles[0].mesh;
    mesh.position_words = [0, 0, 1.0_f32.to_bits()];
    mesh.angle_words = [0; 3];
    mesh.billboard = false;
    mesh.blend = 1;
    mesh.force_blend = true;
    mesh.late_pass = true;
    mesh.set_alpha(255);
    for (vertex, position) in mesh.vertices.iter_mut().zip([
        [-1.0_f32, 1.0, 0.0],
        [1.0, 1.0, 0.0],
        [-1.0, -1.0, 0.0],
        [1.0, -1.0, 0.0],
    ]) {
        vertex.position_words = position.map(f32::to_bits);
    }
    vm.independent_views[0].position_words = [0; 3];
    vm.native_scene_workers
        .insert(0, crate::CmvsNativeSceneWorker::MeshSnow(snow));
    let mut scene = gpu_scene();
    let mut commands = scene
        .native_mesh_commands(&vm, &archive, (32, 32), [1, 0, 0, 0])
        .unwrap();
    commands.insert(
        0,
        solid_quad(
            "red-background",
            0.5,
            [0.5, 0.0, 0.0, 1.0],
            MeshDepth2D::Disabled,
        ),
    );
    let frame = scene.draw(32, 32, commands).unwrap();
    for (actual, expected) in center(&frame).iter().zip([128_u8, 0, 128]) {
        assert!(actual.abs_diff(expected) <= 2, "{actual} != {expected}");
    }
}

fn kind53_meshes(vm: &mut CmvsPs2aVmState) {
    stage(vm);
    let mut owner = crate::CmvsNativeSceneKind53::new([16; 2]).unwrap();
    owner.request(1);
    owner.advance(0, 0).unwrap();
    vm.independent_views[0].position_words = [0.0_f32, 3.2, 0.0].map(f32::to_bits);
    vm.native_scene_workers
        .insert(0, crate::CmvsNativeSceneWorker::Scene53(owner));
}

fn kind53_owner(vm: &mut CmvsPs2aVmState) -> &mut crate::CmvsNativeSceneKind53 {
    let crate::CmvsNativeSceneWorker::Scene53(owner) = vm.native_scene_workers.get_mut(&0).unwrap()
    else {
        panic!("kind53 fixture owner was replaced");
    };
    owner
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_kind53_transition_lifecycle_keeps_base_pixels_and_source_alpha() {
    let (root, archive, mut vm) = fixture();
    for name in crate::CmvsNativeSceneKind53::FALLBACK_RESOURCES {
        write_image(
            root.path(),
            std::str::from_utf8(name).unwrap(),
            16,
            16,
            [255, 0, 0, 128],
        );
    }
    for name in ["try007d1.pb3", "try007d2.pb3"] {
        write_image(root.path(), name, 16, 16, [0, 255, 0, 255]);
    }
    kind53_meshes(&mut vm);
    let mut scene = gpu_scene();
    let mut baseline = None;
    // Native1034==2 selects pipeline8 even while1033 holds shader-kind2.
    // A mask effect or overlapping fade would change these actual GPU pixels.
    for delta in [0, 1250, 1250, 2499, 1] {
        kind53_owner(&mut vm).prepare_draw(delta).unwrap();
        let commands = scene
            .native_mesh_commands(&vm, &archive, (32, 32), [2, 0, 0, 0])
            .unwrap();
        let frame = scene.draw(32, 32, commands).unwrap();
        for (actual, expected) in center(&frame).iter().zip([128_u8, 0, 0]) {
            assert!(actual.abs_diff(expected) <= 2, "{actual} != {expected}");
        }
        if let Some(expected) = &baseline {
            assert_eq!(&frame.rgba8, expected);
        } else {
            baseline = Some(frame.rgba8.clone());
        }
    }
    assert_eq!(kind53_owner(&mut vm).status(), 10);
    kind53_owner(&mut vm).advance(0, 0).unwrap();
    assert_eq!(kind53_owner(&mut vm).status(), 0);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_kind53_missing_secondary_does_not_hide_or_replace_base() {
    let (root, archive, mut vm) = fixture();
    for name in crate::CmvsNativeSceneKind53::FALLBACK_RESOURCES {
        write_image(
            root.path(),
            std::str::from_utf8(name).unwrap(),
            16,
            16,
            [255, 0, 0, 255],
        );
    }
    kind53_meshes(&mut vm);
    let mut scene = gpu_scene();
    // No d-provider files exist. They are allocated by the native lifecycle
    // but cannot become a projective material or replace the retained c-image.
    for delta in [0, 2500, 2500] {
        kind53_owner(&mut vm).prepare_draw(delta).unwrap();
        let commands = scene
            .native_mesh_commands(&vm, &archive, (32, 32), [2, 0, 0, 0])
            .unwrap();
        let frame = scene.draw(32, 32, commands).unwrap();
        assert_eq!(center(&frame), [255, 0, 0]);
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_kind53_visibility_and_late_pass_preserve_native_depth_behavior() {
    let (root, archive, mut vm) = fixture();
    for name in crate::CmvsNativeSceneKind53::FALLBACK_RESOURCES {
        write_image(
            root.path(),
            std::str::from_utf8(name).unwrap(),
            16,
            16,
            [255, 0, 0, 255],
        );
    }
    kind53_meshes(&mut vm);
    kind53_owner(&mut vm).prepare_draw(5000).unwrap();
    kind53_owner(&mut vm).advance(0, 0).unwrap();
    let mut scene = gpu_scene();
    for (selector, expected) in [(3, [255, 0, 0]), (2, [0, 0, 0]), (3, [255, 0, 0])] {
        let owner = kind53_owner(&mut vm);
        owner.request(selector);
        owner.advance(0, 0).unwrap();
        let commands = scene
            .native_mesh_commands(&vm, &archive, (32, 32), [2, 0, 0, 0])
            .unwrap();
        let frame = scene.draw(32, 32, commands).unwrap();
        assert_eq!(center(&frame), expected);

        let mut commands = scene
            .native_mesh_commands(&vm, &archive, (32, 32), [2, 0, 0, 0])
            .unwrap();
        commands.push(solid_quad(
            "later-far-blue",
            0.99999,
            [0.0, 0.0, 1.0, 1.0],
            MeshDepth2D::ReadWriteLess,
        ));
        let frame = scene.draw(32, 32, commands).unwrap();
        assert_eq!(center(&frame), [0, 0, 255]);
    }
}
