use super::*;
use roukaku_media_core::Transform2D;

/// The monitor-independent fullscreen branch of `429370`. BASE is the
/// script/input extent, MODE is the native scene extent, and DWORD52/53 is
/// the configured fullscreen target. Host raster mapping remains StageCanvas's.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NativeViewport {
    base: [u32; 2],
    mode: [u32; 2],
    output: [u32; 2],
    origin: [u32; 2],
}

impl NativeViewport {
    pub(crate) fn from_vm(vm: &CmvsPs2aVmState) -> Result<Option<Self>, CoreError> {
        if !vm.native_view.fullscreen || vm.native_view.suppress_alternate {
            return Ok(None);
        }
        let output = vm
            .native_view
            .fullscreen_size
            .ok_or_else(|| error("SHINKI_EMU_CMVS_NATIVE_FULLSCREEN_MODE_UNBOUND"))?;
        let viewport = Self {
            base: vm.native_stage_size,
            mode: vm.native_render_size,
            output,
            origin: vm.native_view.geometry_origin(),
        };
        if viewport
            .base
            .iter()
            .chain(&viewport.mode)
            .chain(&viewport.output)
            .any(|value| !(1..=16384).contains(value))
            || (0..2).any(|axis| {
                viewport.origin[axis]
                    .checked_add(viewport.mode[axis])
                    .is_none_or(|edge| edge > viewport.output[axis])
            })
        {
            return Err(error("SHINKI_EMU_CMVS_NATIVE_VIEWPORT"));
        }
        Ok(Some(viewport))
    }

    pub(crate) fn extent(self) -> Extent2D {
        Extent2D::new(self.output[0], self.output[1])
    }

    /// `42A0D0` installs this native D3D viewport before `425F40`.
    /// It bounds rasterization even when an individual quad leaves BASE.
    pub(crate) fn clip_rect(self) -> RectI {
        RectI {
            x: self.origin[0] as i32,
            y: self.origin[1] as i32,
            width: self.mode[0],
            height: self.mode[1],
        }
    }

    pub(crate) fn stage_to_view_transform(self) -> Transform2D {
        // `445010` and `466290` apply the width reference scalar to both
        // axes. Pointer4290D0 independently retains BASE-height/MODE-height.
        let scale = self.mode[0] as f32 / self.base[0] as f32;
        Transform2D {
            m11: scale,
            m22: scale,
            // `42C330` adds the effective origin before MODE-normalized
            // pass-through VS output; `42A0D0`'s D3D viewport adds it again.
            tx: 2.0 * self.origin[0] as f32,
            ty: 2.0 * self.origin[1] as f32,
            ..Transform2D::IDENTITY
        }
    }

    /// Input is native client/viewport coordinates after undoing Host and
    /// StageCanvas mappings. `4290D0` uses signed integer division, truncating
    /// toward zero. Native border coordinates remain outside the scene.
    /// This subtracts one origin exactly as native does, even though native
    /// drawing has both the vertex-origin and D3D-viewport contributions.
    pub(crate) fn inverse_view_pointer(self, point: [f32; 2]) -> [i32; 2] {
        std::array::from_fn(|axis| {
            let client = point[axis] as i32;
            ((i64::from(client) - i64::from(self.origin[axis])) * i64::from(self.base[axis])
                / i64::from(self.mode[axis])) as i32
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(vm: &mut CmvsPs2aVmState, command: u16, args: &[u32]) {
        vm.dispatch_stopped = false;
        let span = crate::CmvsPs2aSourceSpan {
            offset: 0,
            byte_length: 2,
        };
        for value in args.iter().rev() {
            crate::vm::execute_cmvs390_frame(
                vm,
                &crate::CmvsPs2aInstructionFrame::Control(
                    crate::CmvsPs2aControlInstruction::PushImmediate {
                        span,
                        stack_bytes: 4,
                        value: *value,
                    },
                ),
            )
            .unwrap();
        }
        crate::vm::execute_cmvs390_frame(
            vm,
            &crate::CmvsPs2aInstructionFrame::Command {
                span,
                command_id: command,
            },
        )
        .unwrap();
    }

    fn fixture() -> CmvsPs2aVmState {
        let mut vm = CmvsPs2aVmState::new(0);
        vm.native_stage_size = [8, 4];
        vm.native_render_size = [8, 4];
        vm.native_view.fullscreen_size = Some([12, 8]);
        run(&mut vm, 349, &[0, 0, 1, 1, 8, 4, 8, 4]);
        run(&mut vm, 386, &[0, u32::MAX, 0, 0, 1, 1]);
        run(&mut vm, 401, &[0, 0, 0, 0, 1, 1]);
        run(&mut vm, 402, &[0, 0]);
        run(&mut vm, 713, &[0]);
        run(&mut vm, 356, &[1]);
        vm
    }

    #[test]
    fn native_unbound_fullscreen_mode_is_an_error_only_for_the_eligible_branch() {
        let mut vm = fixture();
        vm.native_view.fullscreen_size = None;
        assert_eq!(
            NativeViewport::from_vm(&vm).unwrap_err().code(),
            "SHINKI_EMU_CMVS_NATIVE_FULLSCREEN_MODE_UNBOUND"
        );
        run(&mut vm, 279, &[0, 1]);
        assert!(NativeViewport::from_vm(&vm).unwrap().is_none());
        run(&mut vm, 356, &[0]);
        run(&mut vm, 279, &[0, 0]);
        assert!(NativeViewport::from_vm(&vm).unwrap().is_none());
    }

    #[test]
    #[ignore = "requires a hardware GPU"]
    fn gpu_native_center_alternate_and_policy_changes_share_pointer_inverse() {
        let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
        let mut scene = CmvsScene::new(&instance).unwrap();
        let mut vm = fixture();
        let color = [192, 64, 128, 255];
        let image = TextureFrame::from_vec(1, 1, color.to_vec()).unwrap();
        let check = |scene: &mut CmvsScene,
                     vm: &mut CmvsPs2aVmState,
                     expected: [usize; 2],
                     expected_size: [usize; 2]| {
            let commands = vec![SceneCommand::Texture {
                id: "fixture:native-viewport".into(),
                frame: image.clone(),
                destination: RectI {
                    x: 1,
                    y: 1,
                    width: 1,
                    height: 1,
                },
                opacity: 1.0,
                blend: BlendMode::Alpha,
            }];
            let frame = scene.render_overlays(vm, 8, 4, None, commands).unwrap();
            assert_eq!([frame.width as usize, frame.height as usize], expected_size);
            for y in 0..expected_size[1] {
                for x in 0..expected_size[0] {
                    let offset = (y * expected_size[0] + x) * 4;
                    assert_eq!(
                        &frame.rgba8[offset..offset + 4],
                        if [x, y] == expected {
                            &color
                        } else {
                            &[0, 0, 0, 255]
                        }
                    );
                }
            }
            let host = StageCanvas::new(
                Extent2D::new(8, 4),
                Extent2D::new(frame.width, frame.height),
            )
            .unwrap();
            let inverse = host.raster_to_logical_transform();
            let point = [
                (expected[0] as f32 + 0.25) * inverse.m11 + inverse.tx,
                (expected[1] as f32 + 0.25) * inverse.m22 + inverse.ty,
            ];
            let [x, y] = scene.native_pointer(vm, point).unwrap();
            vm.pointer_x = x;
            vm.pointer_y = y;
            vm.native_pointer_present = true;
            run(vm, 404, &[0]);
            assert_eq!([x, y], if expected == [5, 5] { [3, 3] } else { [1, 1] });
            // Native4290D0 subtracts only one origin; preserve its literal
            // asymmetry rather than inventing a visual hit correction.
            assert_eq!(
                vm.interpreter_words[&81236],
                if expected == [5, 5] { u32::MAX } else { 0 }
            );
        };
        check(&mut scene, &mut vm, [5, 5], [12, 8]);
        run(&mut vm, 748, &[1, 1]);
        check(&mut scene, &mut vm, [1, 1], [12, 8]);
        run(&mut vm, 748, &[1, 0]);
        check(&mut scene, &mut vm, [5, 5], [12, 8]);
        run(&mut vm, 765, &[2]);
        check(&mut scene, &mut vm, [5, 5], [12, 8]);
        run(&mut vm, 713, &[1]);
        check(&mut scene, &mut vm, [1, 1], [8, 4]);
        run(&mut vm, 713, &[0]);
        check(&mut scene, &mut vm, [5, 5], [12, 8]);
    }

    #[test]
    #[ignore = "requires a hardware GPU"]
    fn gpu_native_viewport_host_letterbox_has_matching_production_pointer_inverse() {
        let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
        let mut scene = CmvsScene::new(&instance).unwrap();
        scene.resize_raster(24, 24).unwrap();
        let mut vm = fixture();
        let color = [192, 64, 128, 255];
        let edge_color = [64, 192, 128, 255];
        let commands = vec![
            SceneCommand::Texture {
                id: "fixture:native-viewport-bars".into(),
                frame: TextureFrame::from_vec(1, 1, color.to_vec()).unwrap(),
                destination: RectI {
                    x: 1,
                    y: 1,
                    width: 1,
                    height: 1,
                },
                opacity: 1.0,
                blend: BlendMode::Alpha,
            },
            SceneCommand::Texture {
                // Native NDC accepts local -2 after the vertex-origin +2.
                // A BASE clip before projection would incorrectly remove it.
                id: "fixture:native-viewport-active-edge".into(),
                frame: TextureFrame::from_vec(1, 1, edge_color.to_vec()).unwrap(),
                destination: RectI {
                    x: -2,
                    y: -2,
                    width: 1,
                    height: 1,
                },
                opacity: 1.0,
                blend: BlendMode::Alpha,
            },
            SceneCommand::Texture {
                // Native NDC rejects this local -3 edge; merely clipping to the
                // fullscreen target would incorrectly paint its left border.
                id: "fixture:native-viewport-rejected-edge".into(),
                frame: TextureFrame::from_vec(1, 1, edge_color.to_vec()).unwrap(),
                destination: RectI {
                    x: -3,
                    y: 1,
                    width: 1,
                    height: 1,
                },
                opacity: 1.0,
                blend: BlendMode::Alpha,
            },
        ];
        let frame = scene
            .render_overlays(&mut vm, 8, 4, None, commands)
            .unwrap();
        assert_eq!([frame.width, frame.height], [24, 24]);
        for y in 0..24 {
            for x in 0..24 {
                let offset = (y * 24 + x) * 4;
                assert_eq!(
                    &frame.rgba8[offset..offset + 4],
                    if (10..12).contains(&x) && (14..16).contains(&y) {
                        &color
                    } else if (4..6).contains(&x) && (8..10).contains(&y) {
                        &edge_color
                    } else {
                        &[0, 0, 0, 255]
                    }
                );
            }
        }
        // Host first maps its BASE8x4 into the24x24 raster (y6..18),
        // while the native12x8 viewport maps into y4..20. Both inverses
        // must be traversed, rather than treating either bar as native.
        let host = StageCanvas::new(Extent2D::new(8, 4), Extent2D::new(24, 24)).unwrap();
        let inverse = host.raster_to_logical_transform();
        let point = [
            10.5 * inverse.m11 + inverse.tx,
            14.5 * inverse.m22 + inverse.ty,
        ];
        let [x, y] = scene.native_pointer(&vm, point).unwrap();
        assert_eq!([x, y], [3, 3]);
        vm.pointer_x = x;
        vm.pointer_y = y;
        vm.native_pointer_present = true;
        run(&mut vm, 404, &[0]);
        assert_eq!(vm.interpreter_words[&81236], u32::MAX);
        let border = [
            0.5 * inverse.m11 + inverse.tx,
            4.5 * inverse.m22 + inverse.ty,
        ];
        let [x, y] = scene.native_pointer(&vm, border).unwrap();
        vm.pointer_x = x;
        vm.pointer_y = y;
        run(&mut vm, 404, &[0]);
        assert_eq!(vm.interpreter_words[&81236], u32::MAX);
    }

    #[test]
    #[ignore = "requires a hardware GPU"]
    fn gpu_native_mode_aspect_uses_width_scale_but_axis_specific_pointer_divisors() {
        let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
        let mut scene = CmvsScene::new(&instance).unwrap();
        let mut vm = fixture();
        // Independently bound native extents model a negotiated adapter mode;
        // archive startup leaves this mode unbound without that real input.
        vm.native_render_size = [8, 6];
        vm.native_view.fullscreen_size = Some([12, 10]);
        run(&mut vm, 356, &[1]);
        let color = [192, 64, 128, 255];
        let commands = vec![SceneCommand::Texture {
            id: "fixture:native-viewport-mode-aspect".into(),
            frame: TextureFrame::from_vec(1, 1, color.to_vec()).unwrap(),
            destination: RectI {
                x: 1,
                y: 1,
                width: 1,
                height: 1,
            },
            opacity: 1.0,
            blend: BlendMode::Alpha,
        }];
        let frame = scene
            .render_overlays(&mut vm, 8, 4, None, commands)
            .unwrap();
        assert_eq!([frame.width, frame.height], [12, 10]);
        for y in 0..10 {
            for x in 0..12 {
                let offset = (y * 12 + x) * 4;
                assert_eq!(
                    &frame.rgba8[offset..offset + 4],
                    if [x, y] == [5, 5] {
                        &color
                    } else {
                        &[0, 0, 0, 255]
                    }
                );
            }
        }
        let host = StageCanvas::new(Extent2D::new(8, 4), Extent2D::new(12, 10)).unwrap();
        let inverse = host.raster_to_logical_transform();
        let point = [
            5.25 * inverse.m11 + inverse.tx,
            5.25 * inverse.m22 + inverse.ty,
        ];
        let [x, y] = scene.native_pointer(&vm, point).unwrap();
        assert_eq!([x, y], [3, 2]);
        vm.pointer_x = x;
        vm.pointer_y = y;
        vm.native_pointer_present = true;
        run(&mut vm, 404, &[0]);
        assert_eq!(vm.interpreter_words[&81236], u32::MAX);
    }
}
