use super::*;
use crate::{ScOpcodeCatalog, parse_sc};

#[test]
fn particle_mechanisms_resume_in_memory_state_and_finish_fading() {
    for (start, stop) in [
        ("effect Firefly glow 20 1000", "effect end"),
        ("effect2 SnowH", "effect2 fadeout"),
        ("effect2 Snow", "effect2 fadeout"),
        ("effect Snow", "effect2 fadeout"),
    ] {
        let source = format!(".{start}\r\n.{stop}\r\n.end\r\n");
        let make_vm = || {
            MusicaVm::new(
                "musica:/scr/test.sc".into(),
                Hash256::from_sha256(source.as_bytes()),
                parse_sc(source.as_bytes(), &ScOpcodeCatalog::observed_musica()).unwrap(),
                1,
            )
            .unwrap()
        };
        let mut original = make_vm();
        original.step(1).unwrap();
        MusicaVm::checked_test_state(original.state()).unwrap();
        original.advance_firefly_clock(32_000_000).unwrap();
        original.advance_secondary_effect_clock(32_000_000).unwrap();
        let saved = MusicaVm::checked_test_state(original.state()).unwrap();
        let mut restored = make_vm();
        restored.install_test_state(&saved, 2).unwrap();
        original.advance_firefly_clock(16_000_000).unwrap();
        restored.advance_firefly_clock(16_000_000).unwrap();
        original.advance_secondary_effect_clock(16_000_000).unwrap();
        restored.advance_secondary_effect_clock(16_000_000).unwrap();
        assert_eq!(original.state().firefly, restored.state().firefly);
        assert_eq!(
            original.state().secondary_effect,
            restored.state().secondary_effect
        );
        assert_eq!(original.state().random_state, restored.state().random_state);
        original.step(2).unwrap();
        original.advance_firefly_clock(64_000_000).unwrap();
        original.advance_secondary_effect_clock(64_000_000).unwrap();
        assert!(original.state().firefly.is_none());
        assert!(original.state().secondary_effect.is_none());
        MusicaVm::checked_test_state(original.state()).unwrap();
    }
}
