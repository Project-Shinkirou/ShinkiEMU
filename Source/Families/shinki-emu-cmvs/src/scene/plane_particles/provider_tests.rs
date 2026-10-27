use super::*;
use crate::scene::parent_load_tests::{command, fixture};

fn write_atlas(root: &std::path::Path, name: &str, color: impl Fn(usize, usize) -> [u8; 4]) {
    let (width, height) = (1536usize, 768usize);
    let (blocks_x, blocks_y) = (width / 16, height / 16);
    let blocks = blocks_x * blocks_y;
    let flags = blocks.div_ceil(8);
    let channel_size = 12 + flags + blocks;
    let data1 = 84usize;
    let data2 = data1 + 16 + 4 * channel_size;
    let mut source = vec![0u8; data2 + 16 + 47];
    let length = source.len() as u32;
    source[..4].copy_from_slice(b"PB3B");
    source[4..8].copy_from_slice(&length.to_le_bytes());
    source[0x1c..0x1e].copy_from_slice(&1u16.to_le_bytes());
    source[0x1e..0x20].copy_from_slice(&(width as u16).to_le_bytes());
    source[0x20..0x22].copy_from_slice(&(height as u16).to_le_bytes());
    source[0x22..0x24].copy_from_slice(&32u16.to_le_bytes());
    source[0x2c..0x30].copy_from_slice(&(data1 as u32).to_le_bytes());
    source[0x30..0x34].copy_from_slice(&(data2 as u32).to_le_bytes());
    for channel in 0..4 {
        source[data1 + channel * 4..data1 + channel * 4 + 4]
            .copy_from_slice(&(channel_size as u32).to_le_bytes());
        let header = data1 + 16 + channel * channel_size;
        source[header..header + 4].copy_from_slice(&(flags as u32).to_le_bytes());
        source[header + 12..header + 12 + flags].fill(255);
        for block in 0..blocks {
            let rgba = color(block % blocks_x * 16 / 256, block / blocks_x * 16 / 192);
            source[header + 12 + flags + block] = rgba[[2, 1, 0, 3][channel]];
        }
    }
    std::fs::write(root.join(name), source).unwrap();
}

fn write_image(root: &std::path::Path, name: &str, color: [u8; 4]) {
    write_atlas(root, name, |_, _| color);
}

fn initialized_planes(vm: &mut CmvsPs2aVmState, slot: u32) {
    command(vm, 661, &[slot]);
    for selector in 1..=3 {
        command(vm, 687, &[slot, selector, 0, 0]);
        for _ in 0..18 {
            crate::vm::native_scene_worker::tick(vm, 200).unwrap();
        }
    }
}

#[test]
fn plane_geometry_uses_screen_projection_and_preserves_each_atlas_provider() {
    let mut vm = CmvsPs2aVmState::new(0);
    initialized_planes(&mut vm, 0);
    let owner = &vm.native_scene_workers[&0];
    let mut original = Vec::new();
    for index in 0..3 {
        let basic = owner.particle_basic(index).unwrap();
        let geometry = surface::geometry_with_surfaces(&vm, &basic, None, 1024, true).unwrap();
        original.push(geometry.corners([256, 192]));
        assert_eq!(owner.basic_resource_index(index), Some(index));
        assert_eq!(basic.rect_words.unwrap()[2..], [256, 192]);
        assert_eq!(owner.basic_queue_depth(index, 0.0), Some((1, 5.0)));
    }
    vm.independent_views[0].position_words = [123.0_f32.to_bits(); 3];
    vm.independent_views[0].angle_words = [89.0_f32.to_bits(); 3];
    vm.independent_views[0].field_of_view_word = 1.0_f32.to_bits();
    for (index, corners) in original.iter().enumerate() {
        let basic = vm.native_scene_workers[&0].particle_basic(index).unwrap();
        assert_eq!(
            surface::geometry_with_surfaces(&vm, &basic, None, 1024, true)
                .unwrap()
                .corners([256, 192]),
            *corners
        );
    }
    vm.screen_words.insert(1, 2.0_f32.to_bits());
    let basic = vm.native_scene_workers[&0].particle_basic(0).unwrap();
    assert_ne!(
        surface::geometry_with_surfaces(&vm, &basic, None, 1024, true)
            .unwrap()
            .corners([256, 192]),
        original[0]
    );
}

fn plane_frame(scene: &mut CmvsScene, vm: &CmvsPs2aVmState, archive: &CmvsArchive) -> TextureFrame {
    let mut commands = Vec::new();
    let counts = scene
        .prepare_plane_particles(vm, archive, &mut commands)
        .unwrap();
    scene.limit_plane_particles(vm, counts);
    let mut cursor = 0;
    scene
        .append_plane_particles_before(vm, &mut cursor, None, (1024, 614), &mut commands)
        .unwrap();
    scene.draw(1024, 614, commands).unwrap()
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_three_plane_providers_draw_distinct_colors_without_independent_camera_coupling() {
    let (root, archive, mut vm) = fixture();
    let colors = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]];
    for (name, color) in crate::CmvsNativeSceneKind55::RESOURCES
        .into_iter()
        .zip(colors)
    {
        write_image(root.path(), std::str::from_utf8(name).unwrap(), color);
    }
    initialized_planes(&mut vm, 0);
    let mut scene = CmvsScene::new(crate::scene::tests::gpu_instance()).unwrap();
    scene.clear(1024, 614).unwrap();
    let frame = plane_frame(&mut scene, &vm, &archive);
    assert_eq!(scene.plane_uploads.len(), 3);
    let ids: std::collections::BTreeSet<_> = scene
        .plane_uploads
        .values()
        .map(|upload| upload.texture.as_ref().unwrap().0.as_str())
        .collect();
    assert_eq!(ids.len(), 3);
    for color in colors {
        assert!(
            frame
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[..3] == color[..3])
        );
    }
    vm.independent_views[0].position_words = [100.0_f32.to_bits(); 3];
    vm.independent_views[0].angle_words = [90.0_f32.to_bits(); 3];
    assert_eq!(plane_frame(&mut scene, &vm, &archive).rgba8, frame.rgba8);
    vm.screen_words.insert(1, 2.0_f32.to_bits());
    assert_ne!(plane_frame(&mut scene, &vm, &archive).rgba8, frame.rgba8);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_missing_plane_provider_stays_frozen_until_owner_invalidation_releases_all_uploads() {
    let (root, archive, mut vm) = fixture();
    for name in &crate::CmvsNativeSceneKind55::RESOURCES[..2] {
        write_image(
            root.path(),
            std::str::from_utf8(name).unwrap(),
            [255, 0, 0, 255],
        );
    }
    initialized_planes(&mut vm, 0);
    let mut scene = CmvsScene::new(crate::scene::tests::gpu_instance()).unwrap();
    scene.clear(1024, 614).unwrap();
    let frame = plane_frame(&mut scene, &vm, &archive);
    assert!(scene.plane_uploads[&(0, 2)].texture.is_none());
    write_image(root.path(), "ci001c.pb3", [0, 0, 255, 255]);
    assert_eq!(plane_frame(&mut scene, &vm, &archive).rgba8, frame.rgba8);
    let released: Vec<_> = scene
        .plane_uploads
        .values()
        .filter_map(|upload| upload.texture.as_ref().map(|texture| texture.0.clone()))
        .collect();
    scene.invalidate_plane_particles(0);
    assert!(scene.plane_uploads.is_empty());
    for id in &released {
        assert!(scene.reload_releases.contains(id));
    }
    let changed = plane_frame(&mut scene, &vm, &archive);
    assert!(scene.plane_uploads[&(0, 2)].texture.is_some());
    assert_ne!(changed.rgba8, frame.rgba8);
    command(&mut vm, 672, &[0]);
    plane_frame(&mut scene, &vm, &archive);
    assert!(scene.plane_uploads.is_empty());
    assert!(scene.plane_order.is_empty());
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_plane_atlas_clock_selects_the_native_cell_without_reuploading_provider() {
    let (root, archive, mut vm) = fixture();
    let colors = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255; 4],
    ];
    write_atlas(root.path(), "ci001a.pb3", |_, row| colors[row]);
    command(&mut vm, 661, &[0]);
    command(&mut vm, 687, &[0, 1, 0, 0]);
    for _ in 0..4 {
        crate::vm::native_scene_worker::tick(&mut vm, 200).unwrap();
    }
    let mut scene = CmvsScene::new(crate::scene::tests::gpu_instance()).unwrap();
    scene.clear(1024, 614).unwrap();
    let first = plane_frame(&mut scene, &vm, &archive);
    let id = scene.plane_uploads[&(0, 0)]
        .texture
        .as_ref()
        .unwrap()
        .0
        .clone();
    assert!(
        first
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[..3] == colors[0][..3])
    );
    crate::vm::native_scene_worker::tick(&mut vm, 100).unwrap();
    let second = plane_frame(&mut scene, &vm, &archive);
    assert_eq!(scene.plane_uploads[&(0, 0)].texture.as_ref().unwrap().0, id);
    assert!(
        second
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[..3] == colors[1][..3])
    );
    assert_ne!(first.rgba8, second.rgba8);
}
