use super::*;
use crate::{ScOpcodeCatalog, parse_sc};

fn vm(source: &[u8]) -> MusicaVm {
    MusicaVm::new(
        "musica:/scr/stage.sc".into(),
        Hash256::from_sha256(source),
        parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
        1,
    )
    .unwrap()
}

#[test]
fn native_stage_sequence_and_stand_parameters_survive_save_and_restore() {
    let source = b".stage front.png:* back.png 0 -20 actor.png 640,1685\r\n.end\r\n";
    let mut live = vm(source);
    let Some(MusicaVmEvent::Stage(stage)) = live.step(1).unwrap() else {
        panic!("missing stage event");
    };
    assert_eq!(
        stage.foreground.prefix.as_deref(),
        Some("musica:/bg/front.png")
    );
    assert_eq!(stage.foreground.main, None);
    assert_eq!(stage.reference_position, None);
    assert_eq!(stage.background.as_ref().unwrap().y, -20);
    assert_eq!(stage.stands[0].position, 640);
    assert_eq!(stage.stands[0].resource_parameter, 1685);
    assert_eq!(live.state().stage.as_ref(), Some(&stage));
    let saved = live.encode_native_save().unwrap();
    let decoded = MusicaVm::decode_native_save(&saved).unwrap();
    assert_eq!(decoded.stage.as_ref(), Some(&stage));
    let mut restored = vm(source);
    restored.restore_native_save(&saved, 1).unwrap();
    assert_eq!(restored.state().stage.as_ref(), Some(&stage));
}

#[test]
fn reference_coordinates_keep_full_precision_and_one_trailing_empty_field() {
    let mut machine =
        vm(b".stage front.png 2147483647 -2147483648 back.png 12 -34 actor.png 640 \r\n");
    machine.step(1).unwrap();
    let stage = machine.state().stage.as_ref().unwrap();
    assert_eq!(stage.reference_position, Some([i32::MAX, i32::MIN]));
    assert_eq!(stage.background.as_ref().unwrap().x, 12);
    assert_eq!(stage.stands[0].resource_parameter, 0);
}

#[test]
fn native_comma_foreground_slots_parse_in_draw_order() {
    let source = b".stage WHITE.png:char.png,*,loop.png 0 13 back.png 0 0 noise.png 0\r\n";
    let mut machine = vm(source);
    let Some(MusicaVmEvent::Stage(stage)) = machine.step(1).unwrap() else {
        panic!("missing stage event");
    };
    assert_eq!(
        stage.foreground.prefix.as_deref(),
        Some("musica:/bg/WHITE.png")
    );
    assert_eq!(
        stage.foreground.main.as_deref(),
        Some("musica:/bg/char.png")
    );
    assert_eq!(stage.foreground.second, None);
    assert_eq!(
        stage.foreground.third.as_deref(),
        Some("musica:/bg/loop.png")
    );
    assert_eq!(stage.foreground.plus, None);
    assert_eq!(stage.reference_position, Some([0, 13]));
    assert_eq!(
        stage.background.as_ref().unwrap().resource_uri,
        "musica:/bg/back.png"
    );
    assert_eq!(stage.stands[0].resource_uri, "musica:/st/noise.png");
    assert_eq!(
        stage.foreground.uris_in_order(),
        vec![
            "musica:/bg/char.png",
            "musica:/bg/loop.png",
            "musica:/bg/WHITE.png"
        ]
    );
}

#[test]
fn native_plus_slot_and_hidden_colon_tail() {
    let mut machine = vm(b".stage pre.png:a.png+b.png back.png 0 0\r\n");
    machine.step(1).unwrap();
    let stage = machine.state().stage.as_ref().unwrap();
    assert_eq!(stage.foreground.main.as_deref(), Some("musica:/bg/a.png"));
    assert_eq!(stage.foreground.plus.as_deref(), Some("musica:/bg/b.png"));
    assert_eq!(
        stage.foreground.prefix.as_deref(),
        Some("musica:/bg/pre.png")
    );
    let mut machine = vm(b".stage hidden.png: back.png 0 0\r\n");
    machine.step(1).unwrap();
    let stage = machine.state().stage.as_ref().unwrap();
    assert_eq!(stage.foreground.main, None);
    assert_eq!(
        stage.foreground.prefix.as_deref(),
        Some("musica:/bg/hidden.png")
    );
}

#[test]
fn scan_int_uses_native_leading_integer_semantics() {
    assert_eq!(scan_int("30"), Some(30));
    assert_eq!(scan_int("-20"), Some(-20));
    assert_eq!(scan_int("13"), Some(13));
    assert_eq!(scan_int("5x"), Some(5));
    assert_eq!(scan_int("abc"), None);
    assert_eq!(scan_int(""), None);
    assert_eq!(scan_int("2147483648"), None);
}

#[test]
fn invalid_native_stage_does_not_replace_previous_scene() {
    for invalid in [
        "front.png:*:* back.png 0 0",
        "* back.png 0 0 actor.png 640,1,2",
        "* ../back.png 0 0",
        "* back.png 0 0  ",
    ] {
        let source = format!(".stage * back.png 0 0\r\n.stage {invalid}\r\n");
        let mut machine = vm(source.as_bytes());
        machine.step(1).unwrap();
        let previous = machine.state().stage.clone();
        assert!(
            machine.step(2).is_err(),
            "accepted invalid stage: {invalid}"
        );
        assert_eq!(machine.state().stage, previous);
    }
}

#[test]
fn comma_in_resource_name_is_not_reinterpreted_as_a_stand_parameter() {
    let mut machine = vm(b".stage * back.png 0 0 actor.png,1 640\r\n");
    machine.step(1).unwrap();
    let stand = &machine.state().stage.as_ref().unwrap().stands[0];
    assert_eq!(stand.resource_uri, "musica:/st/actor.png,1");
    assert_eq!(stand.resource_parameter, 0);
}

#[test]
fn armed_transition_snapshots_previous_stage_with_native_duration() {
    let source = b".transition 0 * 30\r\n.stage * a.png 0 0\r\n.stage * b.png 0 0\r\n.end\r\n";
    let mut machine = vm(source);
    // Non-yielding commands chain inside one step: the first step already
    // arms the transition and submits stage a.
    let Some(MusicaVmEvent::Stage(_)) = machine.step(1).unwrap() else {
        panic!("missing first stage event");
    };
    assert!(machine.state().transition_active.is_none());
    let Some(MusicaVmEvent::Stage(_)) = machine.step(2).unwrap() else {
        panic!("missing second stage event");
    };
    let active = machine.state().transition_active.as_ref().unwrap();
    assert_eq!(active.method, 0);
    assert_eq!(active.param, 30);
    assert_eq!(active.duration_ns, 25_600_000_000 / 30);
    assert_eq!(
        active.previous.background.as_ref().unwrap().resource_uri,
        "musica:/bg/a.png"
    );
    let saved = machine.encode_native_save().unwrap();
    let decoded = MusicaVm::decode_native_save(&saved).unwrap();
    assert_eq!(
        decoded
            .transition_active
            .as_ref()
            .map(|transition| transition.param),
        Some(30)
    );
    let mut restored = vm(source);
    restored.restore_native_save(&saved, 4).unwrap();
    assert!(
        !restored
            .advance_transition_clock(active.duration_ns)
            .unwrap()
    );
    assert!(restored.state().transition_active.is_none());
}

#[test]
fn zero_param_or_unsupported_method_completes_without_transition() {
    let mut machine = vm(b".transition 0 * 0\r\n.stage * a.png 0 0\r\n.end\r\n");
    machine.step(1).unwrap();
    machine.step(2).unwrap();
    assert!(machine.state().transition_active.is_none());
    // Methods without an implemented compositor still complete instantly.
    let mut machine = vm(b".transition 7 * 100\r\n.stage * a.png 0 0\r\n.end\r\n");
    machine.step(1).unwrap();
    assert!(machine.state().transition_active.is_none());
    assert_eq!(machine.state().transition.param, 100);
}

#[test]
fn masked_transition_methods_arm_with_native_phase_durations() {
    let mut machine = vm(
        b".transition 1 fadeSide2.png 300\r\n.stage * a.png 0 0\r\n.stage * b.png 0 0\r\n.end\r\n",
    );
    machine.step(1).unwrap();
    machine.step(2).unwrap();
    let active = machine.state().transition_active.as_ref().unwrap();
    assert_eq!(active.method, 1);
    assert_eq!(active.duration_ns, 512 * 100_000_000 / 300);
    let saved = machine.encode_native_save().unwrap();
    assert!(MusicaVm::decode_native_save(&saved).is_ok());

    let mut machine = vm(
        b".transition 18 fadeSide2.png 50\r\n.stage * a.png 0 0\r\n.stage * b.png 0 0\r\n.end\r\n",
    );
    machine.step(1).unwrap();
    machine.step(2).unwrap();
    let active = machine.state().transition_active.as_ref().unwrap();
    assert_eq!(active.method, 18);
    assert_eq!(active.duration_ns, 256 * 100_000_000 / 50);

    // Masked methods without a rule image stay unsupported.
    let mut machine = vm(b".transition 1 * 300\r\n.stage * a.png 0 0\r\n.end\r\n");
    machine.step(1).unwrap();
    assert!(machine.state().transition_active.is_none());
}

#[test]
fn image_animation_clock_advances_and_loops() {
    let interval = 1_000_000_000 / 12;
    let mut animations = std::collections::BTreeMap::new();
    animations.insert(
        "musica:/bg/loop.png".to_owned(),
        MusicaImageAnimationState {
            frame_index: 0,
            frame_count: 24,
            interval_ns: interval,
            elapsed_ns: 0,
        },
    );
    let mut machine = vm(b".stage WHITE.png:*,*,loop.png 0 13 back.png 0 0\r\n.end\r\n");
    machine.attach_image_animations(animations).unwrap();
    assert!(!machine.advance_image_animation_clock(1).unwrap());
    assert!(machine.advance_image_animation_clock(interval).unwrap());
    let animation = machine
        .state()
        .image_animations
        .get("musica:/bg/loop.png")
        .unwrap();
    assert_eq!(animation.frame_index, 1);
    assert!(
        machine
            .advance_image_animation_clock(23 * interval)
            .unwrap()
    );
    let animation = machine
        .state()
        .image_animations
        .get("musica:/bg/loop.png")
        .unwrap();
    assert_eq!(animation.frame_index, 0);
}

#[test]
fn restore_rejects_invalid_stage_before_changing_live_state() {
    let source = b".stage * back.png 0 0\r\n.end\r\n";
    let mut live = vm(source);
    live.step(1).unwrap();
    let previous = live.state().clone();
    for variant in 0..4 {
        let mut invalid = previous.clone();
        let stage = invalid.stage.as_mut().unwrap();
        match variant {
            0 => stage.foreground.main = Some("musica:/bg/../private.png".into()),
            1 => {
                stage.stands.resize_with(11, || MusicaStandLayer {
                    resource_uri: "musica:/st/a.png".into(),
                    position: 0,
                    resource_parameter: 0,
                });
            }
            2 => {
                stage.background = Some(MusicaStageLayer {
                    resource_uri: "musica:/bg/*".into(),
                    x: 0,
                    y: 0,
                });
            }
            _ => invalid.schema = "shinki.emu.musica.runtime_state.v26".into(),
        }
        let bytes = postcard::to_allocvec(&invalid).unwrap();
        assert!(MusicaVm::decode_native_save(&bytes).is_err());
        assert!(live.restore_native_save(&bytes, 2).is_err());
        assert_eq!(live.state(), &previous);
    }
}

#[test]
fn ani_patch_clock_walks_eye_sequence_and_flaps_mouth_on_level() {
    let source = b".stage * back.png 0 0\r\n.end\r\n";
    let mut machine = vm(source);
    let mut patches = std::collections::BTreeMap::new();
    patches.insert(
        "musica:/bg/back.png".to_owned(),
        (vec!["恋".to_owned()], 0x2545F4914F6CDD1Du64),
    );
    machine.attach_ani_patches(patches).unwrap();
    let patch = machine
        .state()
        .ani_patches
        .get("musica:/bg/back.png")
        .unwrap();
    assert_eq!(patch.characters[0].eye_state, 0);
    assert_eq!(patch.characters[0].mouth_state, 0);

    // The default sequence (2) opens with (0, 10): one tick arms the walker.
    assert!(
        machine
            .advance_ani_patch_clock(10_000_000, None, None)
            .unwrap()
    );
    let patch = machine
        .state()
        .ani_patches
        .get("musica:/bg/back.png")
        .unwrap();
    assert_eq!(patch.characters[0].eye_index, 1);

    // Mouth only reacts while the speaker matches and the level clears the
    // native 0x320 permille threshold.
    machine
        .advance_ani_patch_clock(10_000_000, Some("翠"), Some((true, 500)))
        .unwrap();
    let patch = machine
        .state()
        .ani_patches
        .get("musica:/bg/back.png")
        .unwrap();
    assert_eq!(patch.characters[0].mouth_state, 0);

    // A speaker match with a rising level over the threshold opens to mid,
    // then a further rise opens fully.
    machine
        .advance_ani_patch_clock(10_000_000, Some("恋"), Some((true, 800)))
        .unwrap();
    let patch = machine
        .state()
        .ani_patches
        .get("musica:/bg/back.png")
        .unwrap();
    assert_eq!(patch.characters[0].mouth_state, 1);

    machine
        .advance_ani_patch_clock(10_000_000, Some("恋"), Some((true, 900)))
        .unwrap();
    let patch = machine
        .state()
        .ani_patches
        .get("musica:/bg/back.png")
        .unwrap();
    assert_eq!(patch.characters[0].mouth_state, 2);
    // Native semantics: without a playing voice, states 0/1 close while the
    // fully open state is retained until a falling level closes it.
    machine
        .advance_ani_patch_clock(10_000_000, Some("恋"), Some((false, 0)))
        .unwrap();
    let patch = machine
        .state()
        .ani_patches
        .get("musica:/bg/back.png")
        .unwrap();
    assert_eq!(patch.characters[0].mouth_state, 2);

    let saved = machine.encode_native_save().unwrap();
    assert!(MusicaVm::decode_native_save(&saved).is_ok());
}

#[test]
fn animation_save_restores_fractional_clocks_and_rng_continuation() {
    let source = b".stage * back.png 0 0\r\n.end\r\n";
    let mut machine = vm(source);
    machine.step(1).unwrap();
    machine
        .attach_image_animations(std::collections::BTreeMap::from([(
            "musica:/bg/back.png".into(),
            MusicaImageAnimationState {
                frame_index: 3,
                frame_count: 4,
                interval_ns: 30_000_000,
                elapsed_ns: 15_000_000,
            },
        )]))
        .unwrap();
    // Zero is a valid intermediate state of the wrapping native LCG.
    machine
        .attach_ani_patches(std::collections::BTreeMap::from([(
            "musica:/bg/back.png".into(),
            (vec!["speaker".into()], 0),
        )]))
        .unwrap();
    machine
        .advance_ani_patch_clock(5_000_000, None, None)
        .unwrap();
    let saved = machine.encode_native_save().unwrap();
    assert_eq!(
        MusicaVm::decode_native_save(&saved).unwrap(),
        *machine.state()
    );
    let mut restored = vm(source);
    restored.restore_native_save(&saved, 2).unwrap();
    assert_eq!(restored.state(), machine.state());
    for vm in [&mut machine, &mut restored] {
        vm.advance_image_animation_clock(45_000_000).unwrap();
        vm.advance_ani_patch_clock(15_000_000, Some("speaker"), Some((true, 900)))
            .unwrap();
    }
    assert_eq!(restored.state(), machine.state());
    assert_eq!(
        machine.state().image_animations["musica:/bg/back.png"].frame_index,
        1
    );
    assert_eq!(
        machine.state().image_animations["musica:/bg/back.png"].elapsed_ns,
        0
    );
}

#[test]
fn image_animation_large_delta_uses_modular_frame_count() {
    let mut machine = vm(b".end\r\n");
    machine
        .attach_image_animations(std::collections::BTreeMap::from([(
            "musica:/bg/loop.png".into(),
            MusicaImageAnimationState {
                frame_index: 1,
                frame_count: 24,
                interval_ns: 3,
                elapsed_ns: 2,
            },
        )]))
        .unwrap();
    let delta = 1_000_000_000_000_000;
    assert!(machine.advance_image_animation_clock(delta).unwrap());
    let animation = &machine.state().image_animations["musica:/bg/loop.png"];
    assert_eq!(animation.frame_index, ((1 + delta.div_ceil(3)) % 24) as u32);
    assert_eq!(animation.elapsed_ns, (delta + 2) % 3);
}

#[test]
fn active_mask_transition_keeps_submitted_rule_when_pending_settings_change() {
    let source = b".transition 1 rule.png 300\r\n.stage * a.png 0 0\r\n.stage * b.png 0 0\r\n.transition 0 * 0\r\n.wait 1\r\n.end\r\n";
    let mut machine = vm(source);
    for tick in 1..=3 {
        machine.step(tick).unwrap();
    }
    assert!(machine.state().transition.resource.is_none());
    assert_eq!(
        machine
            .state()
            .stage
            .as_ref()
            .unwrap()
            .transition
            .resource
            .as_deref(),
        Some("rule.png")
    );
    let saved = machine.encode_native_save().unwrap();
    let mut restored = vm(source);
    restored.restore_native_save(&saved, 4).unwrap();
    assert_eq!(restored.state(), machine.state());
}

#[test]
fn malformed_animation_saves_do_not_replace_live_state() {
    let source =
        b".transition 1 rule.png 300\r\n.stage * a.png 0 0\r\n.stage * b.png 0 0\r\n.end\r\n";
    let mut live = vm(source);
    live.step(1).unwrap();
    live.step(2).unwrap();
    live.attach_image_animations(std::collections::BTreeMap::from([(
        "musica:/bg/b.png".into(),
        MusicaImageAnimationState {
            frame_index: 0,
            frame_count: 4,
            interval_ns: 10_000_000,
            elapsed_ns: 0,
        },
    )]))
    .unwrap();
    live.attach_ani_patches(std::collections::BTreeMap::from([(
        "musica:/bg/b.png".into(),
        (vec!["speaker".into()], 1),
    )]))
    .unwrap();
    let previous = live.state().clone();
    for variant in 0..18 {
        let mut invalid = previous.clone();
        match variant {
            0 => {
                invalid
                    .transition_active
                    .as_mut()
                    .unwrap()
                    .previous
                    .background
                    .as_mut()
                    .unwrap()
                    .resource_uri = "musica:/bg/../bad.png".into()
            }
            1 => invalid.transition_active.as_mut().unwrap().duration_ns = 0,
            2 => {
                let transition = invalid.transition_active.as_mut().unwrap();
                transition.elapsed_ns = transition.duration_ns;
            }
            3 => invalid.transition_active.as_mut().unwrap().method = 18,
            4 => invalid.transition_active.as_mut().unwrap().param = 0,
            5 => {
                invalid
                    .ani_patches
                    .get_mut("musica:/bg/b.png")
                    .unwrap()
                    .elapsed_ns = 10_000_000
            }
            6 => {
                invalid
                    .ani_patches
                    .get_mut("musica:/bg/b.png")
                    .unwrap()
                    .characters[0]
                    .eye_index = u8::MAX
            }
            7 => {
                invalid
                    .ani_patches
                    .get_mut("musica:/bg/b.png")
                    .unwrap()
                    .characters[0]
                    .eye_sequence = 6
            }
            8 => {
                invalid
                    .ani_patches
                    .get_mut("musica:/bg/b.png")
                    .unwrap()
                    .characters[0]
                    .name = "a".repeat(65)
            }
            9 => {
                invalid
                    .ani_patches
                    .get_mut("musica:/bg/b.png")
                    .unwrap()
                    .characters[0]
                    .eye_countdown = u32::MAX
            }
            10 => {
                invalid
                    .ani_patches
                    .get_mut("musica:/bg/b.png")
                    .unwrap()
                    .characters[0]
                    .mouth_last_tick = 1
            }
            11 => {
                invalid
                    .ani_patches
                    .get_mut("musica:/bg/b.png")
                    .unwrap()
                    .characters[0]
                    .mouth_prev_level = 1001
            }
            12 => {
                invalid
                    .image_animations
                    .get_mut("musica:/bg/b.png")
                    .unwrap()
                    .elapsed_ns = 10_000_000
            }
            13 => {
                invalid
                    .image_animations
                    .get_mut("musica:/bg/b.png")
                    .unwrap()
                    .frame_count = 0
            }
            14 => {
                invalid
                    .image_animations
                    .get_mut("musica:/bg/b.png")
                    .unwrap()
                    .frame_index = 4
            }
            15 => {
                let animation = invalid.image_animations.remove("musica:/bg/b.png").unwrap();
                invalid
                    .image_animations
                    .insert("musica:/bg/../bad.png".into(), animation);
            }
            16 => {
                let patch = invalid.ani_patches.remove("musica:/bg/b.png").unwrap();
                invalid
                    .ani_patches
                    .insert("musica:/sys/b.png".into(), patch);
            }
            _ => {
                let patch = invalid.ani_patches.get_mut("musica:/bg/b.png").unwrap();
                patch.characters.resize(11, patch.characters[0].clone());
            }
        }
        let bytes = postcard::to_allocvec(&invalid).unwrap();
        assert!(
            MusicaVm::decode_native_save(&bytes).is_err(),
            "accepted variant {variant}"
        );
        assert!(
            live.restore_native_save(&bytes, 3).is_err(),
            "restored variant {variant}"
        );
        assert_eq!(live.state(), &previous);
    }
}
