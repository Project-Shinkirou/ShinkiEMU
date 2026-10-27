/// Side effects emitted by one native737 controller callback. The parent
/// applies them before advancing the next slot, preserving native slot order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeSceneSound {
    ///48C920/48B6C0 duplicates the retained static buffer (up to32 voices).
    PlayStatic {
        slot: u8,
        volume: u32,
    },
    ///48C9E0/48B7B0 plays the retained main buffer without rewinding it.
    PlayStaticMain {
        slot: u8,
        volume: u32,
        looping: bool,
    },
    StopStatic {
        slot: u8,
    },
    ///48CAB0 targets the separate streaming bank, not static slot+15.
    StopStream {
        slot: u8,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct NativeSceneFrame {
    pub position_words: Option<[u32; 3]>,
    pub angle_words: Option<[u32; 3]>,
    pub pitch_word: Option<u32>,
    pub yaw_word: Option<u32>,
    pub field_of_view_word: Option<u32>,
    pub audio: [Option<NativeSceneSound>; 4],
}
