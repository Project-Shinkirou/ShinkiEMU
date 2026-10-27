use super::*;
use crate::scene::parent_load_tests::{fixture, write_image};

fn scene() -> CmvsScene {
    let mut scene = CmvsScene::new(crate::scene::tests::gpu_instance()).unwrap();
    scene.clear(32, 32).unwrap();
    scene
}
fn draw(scene: &mut CmvsScene, vm: &CmvsPs2aVmState, archive: &CmvsArchive) -> TextureFrame {
    let commands = scene
        .native_mesh_commands(vm, archive, (32, 32), [2, 0, 0, 0])
        .unwrap();
    scene.draw(32, 32, commands).unwrap()
}
fn pixel(frame: &TextureFrame, x: usize, y: usize) -> &[u8] {
    &frame.rgba8[(y * frame.width as usize + x) * 4..(y * frame.width as usize + x) * 4 + 3]
}
fn show(owner: &mut crate::CmvsNativeSceneKind53) {
    owner.request(3);
    owner.advance(0, 0).unwrap();
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_actual_width_selects_each_base_independently_and_keeps_constructor_pixels() {
    let (root, archive, mut vm) = fixture();
    vm.native_stage_size = [32, 32];
    vm.native_render_size = [32, 32];
    write_image(root.path(), "try007c3.pb3", 512, 16, [255, 0, 0, 255]);
    write_image(root.path(), "try007c4.pb3", 16, 16, [255, 255, 0, 255]);
    write_image(root.path(), "try007c1.pb3", 16, 16, [0, 255, 0, 255]);
    write_image(root.path(), "try007c2.pb3", 16, 16, [0, 0, 255, 255]);
    let mut scene = scene();
    let mut owner = scene.prepare_native_scene53(&vm, &archive, 0).unwrap();
    // Native retains the loaded c3 and fallback c2; the first draw must not
    // decode their newly changed on-disk replacements.
    write_image(root.path(), "try007c3.pb3", 512, 16, [0, 255, 0, 255]);
    write_image(root.path(), "try007c2.pb3", 16, 16, [255, 0, 255, 255]);
    show(&mut owner);
    crate::vm::native_scene_worker::install_scene53(&mut vm, 0, owner).unwrap();
    let frame = draw(&mut scene, &vm, &archive);
    assert_eq!(pixel(&frame, 8, 16), [255, 0, 0]);
    assert_eq!(pixel(&frame, 24, 16), [0, 0, 255]);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_failed_initial_bitmap_uses_native_alternate_without_changing_the_other_base() {
    let (root, archive, mut vm) = fixture();
    vm.native_stage_size = [32, 32];
    vm.native_render_size = [32, 32];
    write_image(root.path(), "try007c4.pb3", 512, 16, [0, 0, 255, 255]);
    write_image(root.path(), "try007c1.pb3", 16, 16, [0, 255, 0, 255]);
    write_image(root.path(), "try007c2.pb3", 16, 16, [255, 255, 0, 255]);
    let mut scene = scene();
    let mut owner = scene.prepare_native_scene53(&vm, &archive, 0).unwrap();
    show(&mut owner);
    crate::vm::native_scene_worker::install_scene53(&mut vm, 0, owner).unwrap();
    let frame = draw(&mut scene, &vm, &archive);
    assert_eq!(pixel(&frame, 8, 16), [0, 255, 0]);
    assert_eq!(pixel(&frame, 24, 16), [0, 0, 255]);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_hidden_meshes_do_not_steal_the_backdrop_and_later_slot_record_budget() {
    let (root, archive, mut vm) = fixture();
    vm.native_stage_size = [1280, 720];
    vm.native_render_size = [1280, 720];
    for name in ["try007c3.pb3", "try007c4.pb3"] {
        write_image(root.path(), name, 512, 16, [255, 0, 0, 255]);
    }
    write_image(root.path(), "__try007a.pb3", 1024, 614, [255, 0, 0, 255]);
    write_image(root.path(), "blue.pb3", 16, 16, [0, 0, 255, 255]);
    let mut scene = scene();
    let owner = scene.prepare_native_scene53(&vm, &archive, 0).unwrap();
    crate::vm::native_scene_worker::install_scene53(&mut vm, 0, owner).unwrap();
    let mut rng = 1;
    let mut snow = crate::CmvsNativeSnow::new(
        b"blue.pb3".to_vec(),
        [
            1.0_f32.to_bits(),
            1.0_f32.to_bits(),
            1000.0_f32.to_bits(),
            1.0_f32.to_bits(),
            8192,
            0,
        ],
        &mut rng,
    )
    .unwrap();
    for particle in &mut snow.particles {
        particle.mesh.set_alpha(0);
    }
    let mesh = &mut snow.particles[8190].mesh;
    mesh.position_words = [0, 0, 1.0_f32.to_bits()];
    mesh.angle_words = [0; 3];
    mesh.billboard = false;
    mesh.set_alpha(255);
    for (vertex, position) in mesh.vertices.iter_mut().zip([
        [-100.0_f32, 100.0, 0.0],
        [100.0, 100.0, 0.0],
        [-100.0, -100.0, 0.0],
        [100.0, -100.0, 0.0],
    ]) {
        vertex.position_words = position.map(f32::to_bits);
    }
    vm.native_scene_workers
        .insert(1, crate::CmvsNativeSceneWorker::MeshSnow(snow));
    // Slot0 admits only its backdrop, so slot1 still admits index8190. The
    // backdrop does not cover this bottom-right native-stage sample.
    let frame = scene
        .render(
            &mut vm,
            CmvsTextureSlot::Parent(0),
            &archive,
            (1280, 720),
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(pixel(&frame, 1200, 650), [0, 0, 255]);
    assert_eq!(pixel(&frame, 20, 20), [255, 0, 0]);
}
