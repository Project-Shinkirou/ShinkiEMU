use super::*;

fn scene() -> CmvsNativeSceneKind50 {
    CmvsNativeSceneKind50::new([b"strips.pb3".to_vec(), b"floor.pb3".to_vec()]).unwrap()
}

fn camera(x: f32, z: f32) -> [u32; 2] {
    [x.to_bits(), z.to_bits()]
}

fn depths(owner: &CmvsNativeSceneKind50) -> [f32; 3] {
    std::array::from_fn(|index| f32::from_bits(owner.mesh(index).unwrap().position_words[2]))
}

#[test]
fn native_quads_keep_xz_strip_winding_uv_orientation_and_world_axes() {
    let owner = scene();
    let rotated_view = [
        0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 9.0, 8.0, 7.0, 1.0,
    ];
    for index in 0..owner.mesh_count() {
        let mesh = owner.mesh(index).unwrap();
        // Camera rotation cannot turn these ground planes into billboards.
        assert_eq!(
            mesh.world_matrix(rotated_view).unwrap(),
            mesh.world_matrix(super::super::native_mesh::identity())
                .unwrap()
        );
        let extent = if index == 3 { 15.0 } else { 5.0 };
        let y = if index == 3 { 0.0 } else { -0.1_f32 };
        let positions = mesh
            .vertices
            .each_ref()
            .map(|vertex| vertex.position_words.map(f32::from_bits));
        assert_eq!(
            positions,
            [
                [-extent, y, extent],
                [extent, y, extent],
                [-extent, y, -extent],
                [extent, y, -extent],
            ]
        );
        assert_eq!(
            mesh.vertices
                .each_ref()
                .map(|vertex| vertex.uv_words.map(f32::from_bits)),
            [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]]
        );
        for triangle in super::super::native_mesh::MESH_TRIANGLES.as_chunks::<3>().0 {
            let [a, b, c] = [
                positions[triangle[0] as usize],
                positions[triangle[1] as usize],
                positions[triangle[2] as usize],
            ];
            let normal_y = (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]);
            assert!(normal_y > 0.0);
        }
        assert_eq!(mesh.kind, 3);
        assert_eq!(mesh.force_blend, index == 3);
        assert!(!mesh.late_pass);
    }
    assert!(owner.mesh(4).is_none());
}

#[test]
fn root_providers_stay_bound_to_their_fixed_submission_nodes() {
    let mut owner = scene();
    for _ in 0..8 {
        owner.advance(camera(4.0, 1000.0)).unwrap();
        let names: Vec<&[u8]> = (0..owner.mesh_count())
            .map(|index| owner.resources[owner.mesh_resource_index(index)].as_slice())
            .collect();
        assert_eq!(
            names,
            [
                b"strips.pb3".as_slice(),
                b"strips.pb3",
                b"strips.pb3",
                b"floor.pb3"
            ]
        );
    }
}

#[test]
fn floor_follows_raw_camera_xz_without_following_y_or_scrolling_uvs() {
    let mut owner = scene();
    assert_eq!(owner.mesh(3).unwrap().position_words, [0; 3]);
    owner.advance(camera(-0.0, -123.0)).unwrap();
    let floor = owner.mesh(3).unwrap();
    assert_eq!(
        floor.position_words,
        [(-0.0_f32).to_bits(), 0, (-123.0_f32).to_bits()]
    );
    assert_eq!(depths(&owner), [0.0, 10.0, 20.0]);
    owner.advance(camera(7.25, 55.0)).unwrap();
    let floor = owner.mesh(3).unwrap();
    assert_eq!(
        &floor
            .world_matrix(super::super::native_mesh::identity())
            .unwrap()[12..15],
        &[7.25, 0.0, 55.0]
    );
    assert_eq!(floor.vertices[3].uv_words.map(f32::from_bits), [1.0, 1.0]);
    owner.advance(camera(-9.5, 55.0)).unwrap();
    assert_eq!(owner.mesh(3).unwrap().vertices[0].uv_words, [0, 0]);
}

#[test]
fn recycling_uses_strict_float_threshold_and_never_rewinds() {
    let mut owner = scene();
    owner.advance(camera(0.0, 10.0)).unwrap();
    assert_eq!(depths(&owner), [0.0, 10.0, 20.0]);
    owner.advance([0, 10.0_f32.to_bits() + 1]).unwrap();
    assert_eq!(depths(&owner), [30.0, 10.0, 20.0]);
    owner.advance(camera(0.0, 20.0)).unwrap();
    assert_eq!(depths(&owner), [30.0, 10.0, 20.0]);
    owner.advance(camera(0.0, -500.0)).unwrap();
    assert_eq!(depths(&owner), [30.0, 10.0, 20.0]);
    owner.advance([0, 20.0_f32.to_bits() + 1]).unwrap();
    assert_eq!(depths(&owner), [30.0, 40.0, 20.0]);
}

#[test]
fn large_camera_jump_recycles_one_plane_per_callback_and_wraps_fixed_ids() {
    let mut owner = scene();
    for expected in [
        [30.0, 10.0, 20.0],
        [30.0, 40.0, 20.0],
        [30.0, 40.0, 50.0],
        [60.0, 40.0, 50.0],
    ] {
        owner.advance(camera(0.0, 1000.0)).unwrap();
        assert_eq!(depths(&owner), expected);
    }
    // Four callbacks, not a while-loop catching the scene up to camera Z.
    assert_eq!(owner.phase_z_word, 40.0_f32.to_bits());
}

#[test]
fn addss_phase_can_stop_advancing_while_the_child_index_keeps_wrapping() {
    let mut owner = scene();
    let phase = 268_435_456.0_f32; // ULP=32; phase+10 rounds back to phase.
    owner.phase_z_word = phase.to_bits();
    owner.advance(camera(0.0, phase + 64.0)).unwrap();
    assert_eq!(
        f32::from_bits(owner.mesh(0).unwrap().position_words[2]),
        phase + 32.0
    );
    assert_eq!(owner.phase_z_word, phase.to_bits());
    owner.advance(camera(0.0, phase + 64.0)).unwrap();
    assert_eq!(
        f32::from_bits(owner.mesh(1).unwrap().position_words[2]),
        phase + 32.0
    );
    assert_eq!(owner.next_plane_index, 2);
}

#[test]
fn private_snapshot_continues_the_phase_instead_of_reconstructing_from_camera() {
    let mut owner = scene();
    owner.advance(camera(3.0, 100.0)).unwrap();
    owner.advance(camera(4.0, 100.0)).unwrap();
    let bytes = serde_json::to_vec(&owner).unwrap();
    let mut restored: CmvsNativeSceneKind50 = serde_json::from_slice(&bytes).unwrap();
    restored.validate().unwrap();
    restored.advance(camera(5.0, 100.0)).unwrap();
    assert_eq!(depths(&restored), [30.0, 40.0, 50.0]);
    restored.advance(camera(6.0, 40.0)).unwrap();
    assert_eq!(depths(&restored), [30.0, 40.0, 50.0]);
    restored.advance(camera(7.0, 100.0)).unwrap();
    assert_eq!(depths(&restored), [60.0, 40.0, 50.0]);
    assert_eq!(
        restored.mesh(3).unwrap().position_words,
        [7.0_f32.to_bits(), 0, 100.0_f32.to_bits()]
    );
}

#[test]
fn resource_fields_and_invalid_live_inputs_fail_before_scene_mutation() {
    assert!(CmvsNativeSceneKind50::new([vec![b'a'; 31], Vec::new()]).is_ok());
    for resources in [
        [vec![b'a'; 32], Vec::new()],
        [Vec::new(), vec![b'b'; 32]],
        [b"a\0b".to_vec(), Vec::new()],
        [Vec::new(), b"a\0b".to_vec()],
    ] {
        assert!(CmvsNativeSceneKind50::new(resources).is_err());
    }
    let mut owner = scene();
    owner.advance(camera(2.0, 15.0)).unwrap();
    for words in [[f32::NAN.to_bits(), 0], [0, f32::INFINITY.to_bits()]] {
        let before = owner.clone();
        assert!(owner.advance(words).is_err());
        assert_eq!(owner, before);
    }
    owner.next_plane_index = 3;
    let before = owner.clone();
    assert!(owner.advance(camera(0.0, 100.0)).is_err());
    assert_eq!(owner, before);
    owner.next_plane_index = 0;
    owner.phase_z_word = f32::NAN.to_bits();
    assert!(owner.validate().is_err());
    owner.phase_z_word = 0;
    owner.meshes[0].vertices[0].uv_words[0] = f32::INFINITY.to_bits();
    assert!(owner.validate().is_err());
}
