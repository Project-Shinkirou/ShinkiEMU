use super::*;

mod version;
use version::VmVersion;

/// Core-private native state. Copied global strings are part of a private
/// game save; source script pools are rebound from verified files instead.
/// One this+0x784 channel-child presenter recovered from the native
/// UI toolkit: the case-490 payload reset, the case-491 eight-entry
/// payload table (`sub_432AF0`) and the case-500 quad slot (`sub_42BC90`/
/// `sub_431290`). Basic-surface parameters live in the texture tree.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct CmvsChannelChildPresenter {
    /// Eight 16-byte table entries from `sub_432AF0`: fields land at
    /// +0x14 (b), +0x18 (sign-extended byte e) and +0x1c (c).
    pub payload_entries: [[u32; 3]; 8],
    /// The parallel-array word d (`payload + (index * 2 + 2) * 8`).
    pub payload_extra: [u32; 8],
    /// The case-490 reset value (sign-extended byte of the argument).
    pub reset_value: i32,
    /// The case-500 quad: `[0]=flag, [4]=u16, [6]=u16, [8]=[0xc]=value`.
    pub quad: Option<[u32; 4]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsPs2aVmState {
    format_version: VmVersion,
    /// An instruction failed or unwound after beginning execution. Only a new
    /// machine or a validated successful snapshot may resume execution.
    pub(super) execution_failed: bool,
    pub program_counter: u32,
    pub current_value: Option<u32>,
    /// Start pc of the instruction being executed; store-side diagnostics
    /// report this instead of the mid-expression program counter.
    pub current_instruction_start: u32,
    pub condition_flag: bool,
    /// Backing bytes of the original zero-initialized interpreter variable
    /// buffer. `stack_cursor_bytes` is the logical stack pointer; this vector
    /// can extend beyond it when a positive frame-local write addresses the
    /// current call frame without moving the stack pointer.
    pub stack_bytes: Vec<u8>,
    pub stack_initialized: Vec<bool>,
    pub stack_cursor_bytes: u32,
    /// Absolute byte bases indexed by the recovered intra-script call depth.
    /// Depth zero is the entry frame. Calls append the stack pointer after the
    /// return address has been pushed; returns remove exactly one base.
    pub call_frame_bases: Vec<u32>,
    /// Version-pinned words copied by recovered command handlers. Keys are
    /// original interpreter object byte offsets, not script-visible
    /// variables.
    pub interpreter_words: BTreeMap<u32, u32>,
    /// Native175 can publish its unwritten stack-local extent on load failure.
    /// The value, when defined, lives only in interpreter_words[81236].
    pub native_audio_extent_indeterminate: bool,
    /// Version-pinned words assigned through recovered CMVS process-global
    /// setters. Keys are original addresses, not game-visible variable ids.
    pub process_global_words: BTreeMap<u32, u32>,
    /// Opaque flag ids mutated through the recovered bitmap helper.
    pub process_flag_bits: BTreeSet<u32>,
    /// Opaque indexed words mutated through the recovered range helper.
    pub process_indexed_words: BTreeMap<u32, u32>,
    /// Opaque raw f32 bit-patterns mutated through the recovered float table.
    pub process_float_words: BTreeMap<u32, u32>,
    /// Bounded Shift-JIS values copied into the native 128-slot string table.
    pub process_string_slots: BTreeMap<u32, Vec<u8>>,
    /// Opaque component fields keyed by packed parent-component and field
    /// offsets from the recovered CMVS 3.90 object graph.
    pub component_words: BTreeMap<u32, u32>,
    /// Ordered core-private segments of interpreter-owned string buffers
    /// written by recovered command handlers. Keys are the original buffer
    /// byte offsets; interpreter prefix contents stay unnamed fields.
    pub interpreter_string_buffers: BTreeMap<u16, Vec<CmvsStringSegment>>,
    /// Owned game-relative source paths, keyed by independent native bank
    /// and slot. Directory paths retain their trailing separator.
    pub resource_search_paths: BTreeMap<CmvsResourceSearchBank, BTreeMap<u8, String>>,
    /// Opaque object identities held in recovered interpreter slot tables.
    /// Keys pack the version-pinned table byte offset and slot index; values
    /// are deterministic identities assigned by the creation order.
    pub slot_objects: BTreeMap<u32, u64>,
    pub(crate) cmv_controllers: BTreeMap<u8, super::cmv::CmvController>,
    /// Recovered effect-channel objects, keyed by channel 0..=7. Cases
    /// 400/401/402/403/405/406 read and write the quad records inside these
    /// objects at the original byte offsets; case 405 reports the inverse of
    /// the quad record's visibility dword.
    pub effect_channels: BTreeMap<u8, CmvsEffectChannel>,
    /// Recovered effect playback table entries, keyed by effect index 0..=11.
    /// Cases 276/277/278 write the record fields at dwords 41/44/45.
    pub effect_playback: BTreeMap<u8, CmvsEffectPlaybackRecord>,
    /// Native sixteen-record timeline, including its active cursor and clock.
    pub(crate) native_timeline: Option<crate::system_save::slot_globals::NativeTimeline>,
    pub(crate) message_history: super::message_history::MessageHistory,
    /// Core-private text objects in native table 750; fonts in table 762
    /// outlive destruction of their corresponding text object.
    pub native_text: BTreeMap<u8, CmvsNativeTextSurface>,
    pub native_text_fonts: BTreeMap<u8, String>,
    pub native_font_owners: BTreeMap<u16, CmvsNativeFontOwner>,
    pub native_default_font: String,
    pub native_glyphs: BTreeMap<u8, CmvsNativeGlyphGraph>,
    /// Main+3164 survives native slot loads; private snapshots retain its data.
    pub native_custom_glyphs: CmvsNativeCustomGlyphManager,
    pub native_text_reveals: BTreeMap<u8, Vec<CmvsNativeTextReveal>>,
    pub native_save_catalog: Option<CmvsNativeSaveCatalog>,
    /// Recovered effect channel child elements, keyed by `(channel, child)`.
    /// Cases 378/379/380/382/383 address one element through `sub_466AE0`.
    pub effect_elements: BTreeMap<(u8, u32), CmvsEffectElement>,
    /// Camera zero dwords keyed by native field index. Missing fields retain
    /// the 457BF0 constructor values; fields 1..3 are world position, not RGB.
    pub screen_words: BTreeMap<u32, u32>,
    /// Addressed camera banks selected by 43C0C0; bank zero uses screen_words.
    pub screen_camera_words: BTreeMap<u32, BTreeMap<u32, u32>>,
    pub independent_views: [super::CmvsIndependentView; 2],
    pub native_scene_workers: BTreeMap<u8, super::CmvsNativeSceneWorker>,
    /// Queue manager DWORD23 (command41): reverse equal-key insertion order.
    pub native_mesh_equal_prepend: bool,
    /// Native renderer mode and script-controlled view flags.
    pub native_view: CmvsNativeViewState,
    /// Persisted dwords of the recovered scene object (`game[818]`, byte
    /// offset 3272). The `424`/`426`/`428`/`430`/`432` queries publish
    /// `(base != 0, base + 1 != 0)` for the five layer groups at base words
    /// `289`/`298`/`301`/`304`/`307`, and the matching `425`/`427`/`429`/
    /// `431`/`433` clear base and `base + 2`. Unwritten words read zero, like
    /// the original zero-initialized object.
    pub scene_words: BTreeMap<u32, u32>,
    /// The recovered case-176 resource channel slot records, keyed by slot
    /// index 0..=5. The fixed 256 and zero fields of the original 52-byte
    /// records are constants and stay out of the snapshot.
    pub resource_channel_slots: BTreeMap<u32, CmvsResourceChannelSlotRecord>,
    /// Specialized state owned by each live case-32 top-level texture
    /// container. Object lifetime remains authoritative in `slot_objects`;
    /// this table retains only the recovered resource/surface fields.
    pub texture_parents: BTreeMap<u8, CmvsTextureParentState>,
    pub(crate) texture_reloads: BTreeMap<u8, super::texture_reload::TextureReload>,
    /// Child textures owned by each live top-level container in the case-32
    /// table. Parent slots are bounded to 256 and child ids to 1024 by the
    /// original helpers. Resource content remains in the VFS/script pool;
    /// this snapshot retains only typed presentation state and references.
    pub texture_children: BTreeMap<u8, BTreeMap<u16, CmvsTextureChildState>>,
    /// Live resource slot membership behind the recovered case-321 clear
    /// handler. Slot contents are unrecovered; only occupancy is retained.
    pub resource_slots: BTreeSet<u8>,
    /// Monotonic identity source for recovered slot objects.
    pub next_slot_object_id: u64,
    /// The opaque construction words of recovered singleton slot objects,
    /// keyed by the packed slot-object key. The case-548 handler inspects
    /// only the first word (`sub_458E50` treats a non-zero first word as a
    /// fault); the rest stay opaque.
    pub slot_object_seed_words: BTreeMap<u32, Vec<u32>>,
    /// Keyed opaque records owned by each recovered six-bank filter chain.
    /// The outer key is the bank; inner values are the four words updated by
    /// case 531. Record allocation is bounded independently of host memory.
    pub filter_chain_records: BTreeMap<u8, BTreeMap<u32, CmvsFilterChainRecord>>,
    /// Record ids in the linked-list insertion order recovered from
    /// `sub_468560`; key ordering is not an equivalent interaction order.
    pub filter_chain_record_order: BTreeMap<u8, Vec<u32>>,
    /// The currently highlighted record for each interactive chain bank.
    pub filter_chain_active_records: BTreeMap<u8, u32>,
    /// The menu lock (`Menu_pump` field 12): the first confirm locks the
    /// current item and reports -10; a confirm on the already-locked item
    /// reports its id. Moving or cancelling clears the lock.
    pub filter_chain_locked_records: BTreeMap<u8, u32>,
    /// Case-528 channel binding for each live interactive-chain bank. The
    /// original constructor retains the selected object from the bounded
    /// case-32 table (`this+481+channel`); retaining the channel is required
    /// to reproduce case-533 apply and case-535 selection presentation.
    pub filter_chain_channels: BTreeMap<u8, u8>,
    /// Monotonic apply revision per filter-chain bank. The original walks the
    /// complete queued list at command 533; retaining the boundary makes the
    /// side effect replayable without inventing meanings for opaque fields.
    pub filter_chain_apply_revisions: BTreeMap<u8, u64>,
    /// The deterministic PRNG state behind the recovered case-212 `rand()`
    /// draw. The original uses the C runtime generator without an explicit
    /// seed in the boot path; the retained state keeps save/restore and
    /// replay reproducible.
    pub prng_state: u32,
    /// Session tick mirrored by the host each frame so VM-level lifecycle
    /// events can carry the same clock as the session diagnostics.
    pub tick: u64,
    /// Deterministic session clock in milliseconds. The runtime host derives
    /// it from the validated fixed tick and delta before dispatch; recovered
    /// handlers that called the original host clock read this value.
    pub session_clock_millis: u32,
    /// Interpreter-owned flag words mutated through recovered OR-setters.
    pub interpreter_flag_words: BTreeMap<u16, u32>,
    /// Words of the recovered global settings object, keyed by version-
    /// pinned byte offsets.
    pub settings_words: BTreeMap<u16, u32>,
    /// Presenters of the this+0x784 channel children, keyed by
    /// (channel, child): the eight-entry payload table (`sub_432AF0`)
    /// and the quad slot (`sub_42BC90`).
    pub channel_child_presenters: BTreeMap<(u32, i32), CmvsChannelChildPresenter>,
    /// Ordered private string references held by recovered string-list
    /// append paths, keyed by version-pinned list ids.
    pub private_string_lists: BTreeMap<u16, Vec<CmvsPs2aPrivateStringReference>>,
    /// The outstanding host storage request, if any. Dispatch stays blocked
    /// until the host resolves it through the runtime provider.
    pub storage_await: Option<CmvsPs2aStorageRequest>,
    /// Native cancel-group held state (input-manager dword 275).
    pub input_cancel_held: bool,
    /// Directional edges for the recovered case-535 selection poll, which
    /// moves the highlighted record on up/down and reads the cancel group.
    pub input_up_edge: bool,
    pub input_down_edge: bool,
    /// True when the running command returned the recovered 0xA000 wait
    /// class: the frame ends, the PC stays on the command word, its
    /// operands stay on the stack and the delay counter increments. The
    /// command re-executes next frame and polls the input snapshot again.
    pub wait_frame: bool,
    /// Native linked-list payloads at interpreter offsets 10500/10504.
    /// Tables and title slots live in their process-global owners instead.
    pub native_system_lists: [Vec<Vec<u8>>; 2],
    /// The open movie object's resolved source name; the media graph lives
    /// in the session, the VM retains only object occupancy.
    pub movie_object: Option<String>,
    /// The movie player's audio-clock frame counter (`player->this[8]`),
    /// updated by the host each tick; command 198 publishes it.
    pub movie_frame: u32,
    pub(crate) texture_presentation: Option<super::texture_presentation::TexturePresentation>,
    /// The active script frame index; frame 0 is the entry script.
    pub current_frame: u16,
    /// Script identities loaded into call frames, keyed by frame index. The
    /// host re-reads and re-validates each script from the mounted VFS on
    /// restore; bytecode itself never enters the snapshot.
    pub script_frames: BTreeMap<u16, crate::CmvsScriptFrameIdentity>,
    /// Decoded instruction start offsets per frame (census walk at
    /// install). The first dispatch pc outside this set is the point
    /// where execution first derailed.
    pub script_valid_starts: BTreeMap<u16, std::collections::BTreeSet<u32>>,
    /// The PS2A name-index table of each loaded script frame, keyed by
    /// frame index. Entries are absolute program counters (control-flow
    /// data, not payload) needed by the `0x10e` expression read proven by
    /// `sub_46D270`.
    pub script_name_indices: BTreeMap<u16, Vec<u32>>,
    /// The payload-free string-length table of each loaded script frame,
    /// keyed by frame index and pool-relative offset. The recovered
    /// case-248 handler reads only these lengths; string content stays
    /// owned by the active script pool.
    pub frame_string_lengths: BTreeMap<u16, BTreeMap<u32, u32>>,
    /// Length tables of frames a root reload replaced, newest last. Global
    /// string slots written before the reload keep referencing their
    /// writer's pool, so the length lookup falls back to these.
    pub retired_string_lengths: Vec<BTreeMap<u32, u32>>,
    /// Pool values of frames a root reload replaced; cross-frame reads
    /// keep resolving through them exactly like the retired lengths.
    pub retired_string_values: Vec<BTreeMap<u32, Vec<u8>>>,
    /// Raw pool-A bytes of each loaded frame. The native tag-0 resolver
    /// (`sub_470D50`) reads `base + offset` with no string-start or bounds
    /// validation, so arbitrary offsets stay readable.
    #[serde(skip)]
    pub frame_string_pool_bytes: BTreeMap<u16, Vec<u8>>,
    /// Byte length of each loaded frame's runtime data segment, the
    /// `sub_4781B0` region published at `this+15100` between the program
    /// section and the string pool. A missing entry blocks segment access;
    /// a zero length makes every access out of bounds, matching a script
    /// without a declared segment.
    pub script_data_segment_sizes: BTreeMap<u16, u32>,
    /// Final bytes outside the segment's complete writable dwords.
    pub script_data_segment_tail: BTreeMap<u16, Vec<u8>>,
    /// Written dwords of each frame's data segment, keyed by byte offset.
    /// Unwritten words read as their loaded initial value, which is zero
    /// for every recovered case; the provider materializes non-zero
    /// initial dwords at script load.
    pub script_data_segment_words: BTreeMap<u16, BTreeMap<u32, u32>>,
    /// Native evaluator assembly buffer; global stores copy its bytes.
    pub message_buffer: Vec<u8>,
    /// Native screen-shake owner at byte3256, created by command 549.
    pub screen_shake: Option<CmvsScreenShakeState>,
    /// The case-696 save snapshot: a bottom-up 24-bpp BMP of the presented
    /// frame at the case-688 dimensions, replaced on each successful
    /// capture and preserved on failure.
    pub save_snapshot: Option<Vec<u8>>,
    /// Native confirmation group 01, held dword 272.
    pub input_confirm_held: bool,
    /// Native key-function groups 08/09 (held dwords 293/296).
    pub input_shift_held: bool,
    pub input_control_held: bool,
    pub input_advance_release: bool,
    pub input_advance_press: bool,
    /// Rebound by the host before dispatch; never carried through snapshots.
    #[serde(skip)]
    pub native_frame_delta_ms: u32,
    #[serde(skip)]
    pub native_audio_playing: u16,
    #[serde(skip)]
    pub native_audio_present: u16,
    #[serde(skip)]
    pub native_stage_size: [u32; 2],
    #[serde(skip)]
    pub native_render_size: [u32; 2],
    #[serde(skip)]
    pub native_pointer_present: bool,
    /// The pc/frame a coroutine resumed through the engine request parked
    /// at (the record table's mutable park slot, `sub_46DF10`'s phase
    /// mechanism): the next engine request resumes the park, not the
    /// record head.
    pub engine_request_park: Option<(u32, u32)>,
    /// True from the engine-request resume until the next fallback entry
    /// captures its park; keeps story parks out of the record slot.
    pub engine_request_live: bool,
    /// True only while the host has actually posted the resume-request
    /// dwords; guards the fallback against stale script words in the
    /// dword range consumed as a request during scheduler-off windows.
    pub engine_request_armed: bool,
    /// True while the system menu session is open (the host's input state
    /// mirrors it): the channel-7 gate mapping only applies inside it.
    pub menu_session_live: bool,
    /// Mirror of the native input manager's selection id (field 347 of
    /// the `this[818]` object): written on a confirmed quad's press edge
    /// (`0x46043d`), read by `cmd 846`. A dedicated VM field — byte 1388
    /// of the interpreter space and process word 2200 are live
    /// script-visible words; mirroring into either clobbered the save
    /// chain.
    pub input_selection_mirror: u32,
    /// One-shot priority arm for the item dispatcher's callback-slot
    /// read: while set, the interception answers with this value instead
    /// of the mirror (which the pointer-quad poll overwrites each pass).
    /// The dispatcher's item dispatch consumes it.
    pub input_selection_override: Option<u32>,
    /// The load-confirm selection the session armed; cmd 846's query
    /// (`sub_45CBB0`, field 347) publishes it into word 81220 while
    /// armed, so the load tail's `[sysreg0 == item-id]` gate matches.
    /// The save flow never arms it and keeps publishing zero.
    pub load_confirm_selection: Option<u32>,
    /// One-shot: the menu render pump (intproc 342684 loop) unwound
    /// through its 349086 exit — the native program switch moment for a
    /// deferred load.
    pub menu_pump_exit_seen: bool,
    pub pointer_x: i32,
    pub pointer_y: i32,
    /// Rebound from verified scripts after restore; never written into saves.
    #[serde(skip)]
    pub frame_string_values: BTreeMap<u16, BTreeMap<u32, Vec<u8>>>,
    pub dispatch_stopped: bool,
    /// Set when a wait command completes (the native engine's frame
    /// boundary); the session refreshes the entry queue before resuming.
    pub wait_boundary_hit: bool,
    /// Dialog dispatches queued by the session's presenter-click
    /// callback; the scheduler drains one entry per frame pass.
    pub pending_dialog_dispatches: Vec<(u32, u16, u32)>,
    /// Set when a queued dialog dispatch has run; the next tick's first
    /// pass parks the message word sentinel to end the cascade.
    pub dialog_dispatch_consumed: bool,
    /// Call-frame depth captured when a queued dispatch resumes; the
    /// tick stops once the returns unwind back to it (the native engine
    /// resumes one entry per frame).
    pub dialog_dispatch_base_depth: Option<usize>,
    /// Stack cursor captured before the dispatch layout was pushed; the
    /// entry-completion unwind restores it so no layout words leak into
    /// the poll family's frame chain.
    pub dialog_dispatch_cursor: Option<u32>,
    /// Record context the native input callback armed for the next
    /// rebuild: (frame word, pc, identity) captured at click time, before
    /// the ring's message path consumes the registration itself. The
    /// identity is laid beneath the frame base like the native callback's
    /// argument frame, so the handler's first locals read it directly.
    pub forced_dialog_record: Option<(u32, u32, u32)>,
}

impl CmvsPs2aVmState {
    pub fn read_interpreter_word(&self, offset: u32) -> Result<u32, CoreError> {
        if offset == 81236 && self.native_audio_extent_indeterminate {
            return Err(CoreError::invalid(
                "SHINKI_EMU_CMVS_NATIVE_AUDIO_EXTENT_INDETERMINATE",
                "native175 published an uninitialized decoder extent",
            ));
        }
        Ok(self.interpreter_words.get(&offset).copied().unwrap_or(0))
    }

    pub fn write_interpreter_word(&mut self, offset: u32, value: u32) -> Option<u32> {
        if offset == 81236 {
            self.native_audio_extent_indeterminate = false;
        }
        self.interpreter_words.insert(offset, value)
    }

    pub(crate) fn native_audio_poll_channel(&self) -> Result<Option<u8>, CoreError> {
        let selector = super::stack::read_stack_word_from_top(self, 4)?;
        Ok(if selector < 4 {
            Some(selector as u8 + 5)
        } else {
            None
        })
    }

    /// True once any instruction failed; the session layer checks this
    /// before each frame without reaching the private field.
    pub fn execution_error(&self) -> Option<&'static str> {
        self.execution_failed
            .then_some("CMVS PS2A VM execution previously failed")
    }

    /// Queues the dialog dispatcher entry the way the native Win32
    /// click callback does: the message identity lands as the dispatcher
    /// call argument and a frame-entry context is appended so the next
    /// scheduler pass resumes the dispatcher chain.
    pub fn queue_dialog_dispatch(&mut self, entry_pc: u32, frame: u16, id: u32) {
        self.pending_dialog_dispatches.push((entry_pc, frame, id));
    }

    /// Hit-tests the pointer against the system-menu channel's quad
    /// table (the same layout the `cmd 404` poll walks). The native
    /// input manager resolves the key-quad id from the Win32 click
    /// message itself; this query gives the session's press handler the
    /// same immediate answer.
    pub fn menu_quad_hit(&self) -> Option<u32> {
        let object = self.effect_channels.get(&7)?;
        let (origin_x, origin_y) = object.origin();
        (0..32_u32)
            .filter(|quad| object.quad_visible(*quad))
            .find(|quad| {
                let [x, y, w, h] = object.quad_rect(*quad);
                let (left, top) = (origin_x.wrapping_add(x), origin_y.wrapping_add(y));
                self.pointer_x >= left
                    && self.pointer_y >= top
                    && self.pointer_x <= left.wrapping_add(w)
                    && self.pointer_y <= top.wrapping_add(h)
            })
    }

    pub fn frame_local_word(&self, offset: i32) -> Option<u32> {
        super::memory::read_frame_local_word(self, offset).ok()
    }

    /// Seeds the interpreter's root frame triple (`this[3306/3305/3304]`)
    /// at the stack base exactly like the native bootstrap, so coroutine
    /// yields that drain the scheduler candidates always land on a valid
    /// return address.
    pub fn seed_root_frame(&mut self) {
        if self.stack_cursor_bytes != 0 {
            return;
        }
        for word in [0u32, self.program_counter, u32::from(self.current_frame)] {
            self.stack_bytes.extend_from_slice(&word.to_le_bytes());
            self.stack_initialized.push(true);
            self.stack_cursor_bytes += 4;
        }
        self.call_frame_bases = vec![self.stack_cursor_bytes];
    }

    pub fn new(program_counter: u32) -> Self {
        Self {
            format_version: VmVersion,
            program_counter,
            current_value: None,
            current_instruction_start: 0,
            condition_flag: false,
            stack_bytes: Vec::new(),
            stack_initialized: Vec::new(),
            stack_cursor_bytes: 0,
            call_frame_bases: vec![0],
            interpreter_words: BTreeMap::from([
                (1552, 1),
                (1556, 1),
                (1452, 1),
                (1464, 0),
                (1472, 1),
                (1476, 0),
                (1480, 1),
                (1484, 2),
                (1488, 4),
                (1496, 0),
                (1500, 0),
                (1504, 10),
                (1508, 0),
                (1512, 0),
                (1516, 0),
                (1520, 1),
                (1596, 1),
                (15064, 0),
            ]),
            native_audio_extent_indeterminate: false,
            process_global_words: BTreeMap::from([
                (super::NATIVE_FONT_SHIFT_JIS, 1),
                (super::native_text::font_owner::NATIVE_FONT_PROPORTIONAL, 0),
            ]),
            process_flag_bits: BTreeSet::new(),
            process_indexed_words: BTreeMap::new(),
            process_float_words: BTreeMap::new(),
            process_string_slots: BTreeMap::new(),
            component_words: BTreeMap::from([
                ((1920_u32 << 16) | 96, 1),
                ((1920_u32 << 16) | 100, 1),
                ((1920_u32 << 16) | 104, 2),
            ]),
            interpreter_string_buffers: BTreeMap::new(),
            resource_search_paths: BTreeMap::new(),
            slot_objects: BTreeMap::new(),
            cmv_controllers: BTreeMap::new(),
            effect_channels: BTreeMap::new(),
            effect_playback: BTreeMap::new(),
            native_timeline: None,
            message_history: super::message_history::MessageHistory::default(),
            native_text: BTreeMap::new(),
            native_text_fonts: BTreeMap::new(),
            native_font_owners: BTreeMap::new(),
            native_default_font: crate::archive::DEFAULT_NATIVE_FONT.to_owned(),
            native_glyphs: BTreeMap::new(),
            native_custom_glyphs: CmvsNativeCustomGlyphManager::default(),
            native_text_reveals: BTreeMap::new(),
            native_save_catalog: None,
            effect_elements: BTreeMap::new(),
            screen_words: BTreeMap::new(),
            screen_camera_words: BTreeMap::new(),
            independent_views: super::independent_view::default_pair(),
            native_scene_workers: BTreeMap::new(),
            native_mesh_equal_prepend: false,
            native_view: CmvsNativeViewState::default(),
            scene_words: BTreeMap::new(),
            resource_channel_slots: BTreeMap::new(),
            texture_parents: BTreeMap::new(),
            texture_children: BTreeMap::new(),
            texture_reloads: BTreeMap::new(),
            resource_slots: BTreeSet::new(),
            next_slot_object_id: 1,
            slot_object_seed_words: BTreeMap::new(),
            filter_chain_records: BTreeMap::new(),
            filter_chain_record_order: BTreeMap::new(),
            filter_chain_active_records: BTreeMap::new(),
            filter_chain_locked_records: BTreeMap::new(),
            filter_chain_channels: BTreeMap::new(),
            filter_chain_apply_revisions: BTreeMap::new(),
            prng_state: 1,
            tick: 0,
            session_clock_millis: 0,
            interpreter_flag_words: BTreeMap::new(),
            settings_words: super::runtime::SETTINGS_RESET_FIELDS.into_iter().collect(),
            channel_child_presenters: BTreeMap::new(),
            private_string_lists: BTreeMap::new(),
            storage_await: None,
            input_cancel_held: false,
            input_up_edge: false,
            input_down_edge: false,
            wait_frame: false,
            native_system_lists: [Vec::new(), Vec::new()],
            movie_object: None,
            movie_frame: 0,
            texture_presentation: None,
            current_frame: 0,
            script_frames: BTreeMap::new(),
            script_valid_starts: BTreeMap::new(),
            script_name_indices: BTreeMap::new(),
            frame_string_lengths: BTreeMap::new(),
            retired_string_lengths: Vec::new(),
            retired_string_values: Vec::new(),
            frame_string_pool_bytes: BTreeMap::new(),
            script_data_segment_sizes: BTreeMap::new(),
            script_data_segment_tail: BTreeMap::new(),
            script_data_segment_words: BTreeMap::new(),
            message_buffer: Vec::new(),
            frame_string_values: BTreeMap::new(),
            screen_shake: None,
            save_snapshot: None,
            input_confirm_held: false,
            input_shift_held: false,
            input_control_held: false,
            input_advance_release: false,
            input_advance_press: false,
            native_frame_delta_ms: 0,
            native_audio_playing: 0,
            native_audio_present: 0,
            native_stage_size: [0, 0],
            native_render_size: [0, 0],
            native_pointer_present: false,
            engine_request_park: None,
            engine_request_live: false,
            engine_request_armed: false,
            menu_session_live: false,
            input_selection_mirror: 0,
            input_selection_override: None,
            load_confirm_selection: None,
            menu_pump_exit_seen: false,
            pointer_x: 0,
            pointer_y: 0,
            dispatch_stopped: false,
            wait_boundary_hit: false,
            pending_dialog_dispatches: Vec::new(),
            dialog_dispatch_consumed: false,
            dialog_dispatch_base_depth: None,
            dialog_dispatch_cursor: None,
            forced_dialog_record: None,
            execution_failed: false,
        }
    }
}
