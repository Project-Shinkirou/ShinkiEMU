//! Version-fingerprinted PS2A command contracts proven from the CMVS 3.90
//! dispatch table and command handlers.
//!
//! A contract is intentionally absent until both its stack consumption and
//! observable effect boundary are static-analysis evidence.  Callers must
//! block on `None`; command ids are not an extension point for guessed
//! behavior.

mod native_text;
mod words;
pub use native_text::CmvsNativeTextOperation;
use words::*;
pub(crate) mod native_table;
mod opcodes_0_71;
mod opcodes_138_322;
mod opcodes_323_354;
mod opcodes_368_401;
mod opcodes_402_529;
mod opcodes_530_741;
mod opcodes_747_940;
mod opcodes_80_137;
type CommandFields = (
    u8,
    CmvsPs2aCommandEffectKind,
    &'static [CmvsPs2aCommandStackWord],
);

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The command result shape consumed by the CMVS 3.90 interpreter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsPs2aCommandEffectKind {
    /// Native bitmap-font sheet load, indexed glyph creation and descriptor update.
    NativeCustomGlyph {
        command_id: u16,
    },
    NativeText {
        operation: CmvsNativeTextOperation,
    },
    ConfigureEffectChannelText {
        operation: CmvsNativeTextOperation,
    },
    /// `sub_483FB0` (case 52) -> `sub_444F10`: on a live channel slot, the
    /// child-state vtable query's result lands in the pinned result word —
    /// the recovered subset reports the child-absent default 0. An
    /// out-of-range or unbound slot raises the mask. Returns 0x4010.
    QueryChannelChildState {
        slot_table_dword_index: u32,
        error_mask: u32,
        result_field_offset: u32,
    },
    /// `sub_483EB0` (case 59): loads an image and native color/filter
    /// parameters into an existing parent at byte table 1924.
    /// Missing parents raise 0x10 without overwriting the result.
    LoadTextureParentFromName {
        texture_table_byte_offset: u32,
        error_mask: u32,
        result_field_offset: u32,
    },
    /// Native 107–111/125 start the plane-camera worker, not a movie.
    StartCameraTransition {
        command_id: u16,
    },
    /// Commands 608–629 address the independent primary view owner.
    NativeIndependentView {
        command_id: u16,
    },
    /// Ordinary constructors for the native table737 scene worker.
    NativeSceneWorker {
        command_id: u16,
    },
    SetNativeQueuePolicy,
    QueryNativeSceneWorkerStatus,
    RequestNativeSceneWorker,
    /// Native image collision geometry816–818.
    NativeHitPolygon {
        command_id: u16,
    },
    /// `sub_47A4C0` (case 710) -> `sub_444CB0`: publishes whether the
    /// channel's transition words (dwords 3100/3101) are active into the
    /// pinned result word; an unbound channel raises 0x10000. Returns
    /// 0x4004.
    QueryChannelTransitionBusy {
        slot_table_dword_index: u32,
        error_mask: u32,
        result_field_offset: u32,
    },
    /// `sub_47A4C0` sibling `sub_47A490` (case 709) -> `sub_444BE0`:
    /// releases the channel's transition sub-objects and clears its
    /// pending words; no interpreter state changes. Returns 0x4004.
    ReleaseChannelTransition,
    /// `sub_4822F0` (case 508): advances the channel child's draw state
    /// through `sub_42BC00` (void); the container/child validation raises
    /// 0x10/0x20 like case 509, but no result word is written. Returns
    /// 0x400C.
    KickTextureChildDraw,
    /// `sub_482130` (case 505): releases the channel child at the stack
    /// index (negative releases the object itself); the container/child
    /// validation raises 0x10/0x20 like case 509. Returns 0x4008.
    ReleaseTextureChildByIndex,
    /// `sub_482220` (case 506): nine-word child geometry call through the
    /// 481-table slot; the slot gate raises 0x10 and an index at or above
    /// 1024 raises 0x20. Returns 0x4014.
    ConfigureTextureChildGeometry,
    /// `sub_482360` (case 507): stores the value word through the
    /// 481-table child; the slot gate raises 0x10 and an index at or
    /// above 1024 raises 0x20. Returns 0x400C.
    StoreTextureChildWord,
    /// `sub_489CB0` (case 568): clamps the consumed word to 0-100 and
    /// applies it as a system volume through `sub_48D720`; no interpreter
    /// state changes. Returns 0x4004.
    SetSystemVolume,
    /// The dispatch table returns a negative result, ending the current
    /// interpreter dispatch cycle. It is not evidence that the entire game
    /// session has reached a terminal state.
    StopDispatch,
    /// Mechanically recovered handler body (see `command::native_table`):
    /// run the static micro program at `(cluster, table_index)`.
    NativeMicroProgram {
        cluster: u8,
        table_index: u8,
    },
    /// Handler `sub_479C90` (case 846): publishes the input manager's
    /// current selection id (`sub_45CBB0`: field 347 of the `this[818]`
    /// object, set by a key byte's press edge, cleared by `cmd 847`).
    /// The port mirrors the menu-session quad id into a dedicated VM
    /// field on the same press edge; the query reads that mirror.
    /// Returns 0x4000.
    QueryProcessSelection {
        result_field_offset: u32,
    },
    /// Case 160: play a resource through the alternating audio channels.
    PlayAudio,
    /// Case 162: clear resource names and fade out the active audio channel.
    FadeOutAudio,
    /// Case 161: stop both audio channels and reset the channel selector.
    StopAudio,
    /// Case 966 (`sub_479060`): resolves the stack-top resource name, opens
    /// the scenario stream, marks the loading flag and suspends the calling
    /// coroutine (`0xC004`) until the load completes. The headless archive
    /// resolves synchronously. A failed or busy open continues without
    /// suspending (`0x4004`).
    OpenScenarioStream {
        loading_field_offset: u32,
    },
    /// Native CImageVideo controller commands, including their five/six-word opens.
    CmvController {
        command_id: u16,
    },
    /// Case 73/193 (`sub_489550`): releases the single object held at dword
    /// index 794 and returns 0x4000, so it consumes nothing.
    ReleaseSingleObject {
        dword_index: u32,
    },
    /// `sub_484F90` resolves the graphics component through `432F10`, then
    /// `4164F0` stores a boolean in its DWORD18 without a reset preamble.
    StoreComponentBoolean {
        component_field_offset: u16,
        field_offset: u16,
    },
    /// Case 349 (`sub_47ED00`): rebuilds the effect playback object for one
    /// of eight effect slots bound to one of twelve playback entries and
    /// consumes six opaque configuration words. Out-of-range channels raise
    /// the 0x10000 mask and out-of-range effects the 0x1000 mask. The media
    /// graph behind the objects is not recovered; occupancy is observable.
    SetEffectChannelEnabled {
        effect_table_dword_index: u32,
        max_channel: u32,
        error_mask: u32,
    },
    /// `sub_47F830` (case 322): forwards one opaque word to an occupied
    /// effect channel; out-of-range or unoccupied channels raise the mask.
    ForwardEffectChannelWord {
        effect_table_dword_index: u32,
        max_channel: u32,
        error_mask: u32,
    },
    /// `sub_47D770` (case 376): forwards one opaque block word to an
    /// occupied effect channel's object.
    ForwardEffectChannelQuad {
        effect_table_dword_index: u32,
        max_channel: u32,
        error_mask: u32,
    },
    /// Case 350: reads the effect engine's current two-word position into
    /// two result fields.
    QueryEffectPosition {
        result_x_field_offset: u32,
        result_y_field_offset: u32,
    },
    /// Case 351: runs the pointer hit test and stores the canonical result.
    QueryEffectPointerHit {
        result_field_offset: u32,
    },
    ForwardEffectChannelPair {
        effect_table_dword_index: u32,
        max_channel: u32,
        error_mask: u32,
    },
    ForwardEffectChannelBlock {
        effect_table_dword_index: u32,
        max_channel: u32,
        error_mask: u32,
    },
    /// `sub_47D850` (case 378): selects one child of an occupied effect
    /// channel by value and forwards a boolean to it.
    SetEffectChildEnabled {
        effect_table_dword_index: u32,
        max_channel: u32,
        error_mask: u32,
    },
    /// `sub_47E690` (case 368): resolves the consumed string reference and
    /// starts it through one of eight sound channels; failures raise 0x80
    /// and channel table faults 0x10000.
    SelectEffectChild {
        effect_table_dword_index: u32,
        max_channel: u32,
        error_mask: u32,
    },
    /// Cases 384-391 (`sub_47E240`/`E2B0`/`E4C0`/`E540`/`E630`/`E3A0`/`E400`/
    /// `E460`): select one child of an occupied effect channel by value and
    /// run a child-specific action over the remaining opaque words. The
    /// channel table faults raise 0x10000; a missing child raises it too.
    SelectEffectQuad {
        error_mask: u32,
    },
    /// `sub_47F640` (case 403) -> `sub_466D90`: hide one of 32 quads on the
    /// occupied channel by clearing the quad record's visibility flag at
    /// byte offset `76*quad + 40`; a channel or selector fault raises
    /// `error_mask`. Returns `0x4008`.
    DeselectEffectQuad {
        error_mask: u32,
    },
    QueryEffectState {
        first_field_offset: u32,
        second_field_offset: u32,
        third_field_offset: u32,
    },
    /// `sub_479C20` (case 416): copy the input manager's advance-group
    /// latches (`sub_45CCB0` reads manager dwords 271/272) into the
    /// interpreter state fields. The first field receives the release
    /// edge and the second the held state. The command takes no stack
    /// operands and returns `0x4000`, so the dispatcher pops nothing.
    ReadInputLatches {
        release_field_offset: u32,
        held_field_offset: u32,
    },
    /// Case 751: read skip/auto input modes or the interpreter override.
    ReadAutomaticAdvance {
        result_field_offset: u32,
        override_field_offset: u32,
    },
    /// `sub_4793B0` (case 751): texture-readiness poll. When the override
    /// field is zero the canonical readiness value is stored; any non-zero
    /// override forces success. No stack operands; returns `0x4000`.
    StoreTextureReadyFlag {
        result_field_offset: u32,
        override_field_offset: u32,
    },
    /// Case 196 (`sub_4891B0`): resolves the movie name, opens the stream
    /// through `sub_412800` and writes the open result into the interpreter
    /// word at the result field. Returns `0x4004`.
    OpenMovieStream {
        result_field_offset: u32,
    },
    /// Case 197 (`sub_489360`): releases the movie object held at dword
    /// index 795 and returns `0x4000`.
    ReleaseMovieObject,
    /// Case 198 (`sub_489330`): publishes the movie object's state into the
    /// interpreter word at the result field, or `-1` when the object is
    /// gone. Returns `0x4000`.
    QueryMovieState {
        result_field_offset: u32,
    },
    /// `sub_484020` (case 51): one script slot selector; occupancy of the
    /// `sub_484020` (case 51): one script slot selector; occupancy of the
    /// slot-table object is required (else the slot error mask raises) and
    /// the presentation state lands in four consecutive interpreter words.
    /// Case 696 (`sub_471030`): captures the presented framebuffer,
    /// downsamples to the dimensions stored by case 688 and retains the
    /// bottom-up 24-bpp BMP as the save snapshot. Returns 0 on success and
    /// on readback failure (no error signal; the old snapshot is kept).
    CaptureSaveSnapshot,
    /// Case 693 (`sub_4859D0`): loads the named save slot and switches the
    /// running program; the host owns the save files.
    LoadNativeSaveSlot,
    /// Case 694 (`sub_485BB0`): writes the running program state into the
    /// named save slot.
    SaveNativeGameSlot,
    /// Case 699 (`sub_485440`): probes whether `saveNNN.dat` exists with a
    /// valid `CSV` magic; the result lands in the interpreter result word.
    ProbeNativeSaveSlot {
        result_field_offset: u32,
    },
    /// Case 700 (`sub_485A60`): rewrites the save header's title field.
    SetNativeSaveTitle {
        result_field_offset: u32,
    },
    /// Case 701 (`sub_4855C0`): reads the save slot's timestamp, title,
    /// metadata and thumbnail; failures land in the result word as zero.
    LoadNativeSaveInfo,
    /// Case 702 (`sub_485BE0`): rewrites the save header's metadata pair.
    SetNativeSaveMeta {
        result_field_offset: u32,
    },
    /// Case 51 (`484020`): provider presence, width, height and source alpha.
    /// Missing slots retain all results and raise the native error flag.
    QueryTextureParentImageInfo {
        slot_table_dword_index: u32,
        error_mask: u32,
        result_field_offset: u32,
    },
    /// `sub_47EFC0` (case 334): changes native scheduler enable (DWORD 1),
    /// clears busy (DWORD 2) when changed, and propagates graphics/text
    /// visibility. Returns `0x4008`; an empty channel raises the error mask.
    SetEffectChannelPlaybackFlag {
        effect_table_dword_index: u32,
        error_mask: u32,
    },
    QueryEffectQuadActive {
        result_field_offset: u32,
    },
    /// `sub_47E770` (case 331): stores whether the effect channel slot is
    /// occupied into the interpreter result field; an out-of-range channel
    /// raises the channel error mask. Returns `0x4004`.
    QueryChannelOccupancy {
        result_field_offset: u32,
        channel_table_dword_index: u32,
        error_mask: u32,
    },
    ConfigureEffectElementBasic {
        command_id: u16,
    },
    /// `sub_47F400` (case 400) -> `sub_466CB0`: one of 32 quads on an occupied
    /// effect channel receives six `__int16` geometry values at a sub-selector
    /// slot; faults raise 0x20000.
    ConfigureEffectQuadGeometry {
        error_mask: u32,
    },
    /// `sub_47F530` (case 401) -> `sub_466D10`: one of 32 quads on an occupied
    /// effect channel receives four `__int16` hit-rectangle words; faults
    /// raise 0x20000.
    ConfigureEffectQuadRect {
        error_mask: u32,
    },
    /// `sub_47FA40` (case 336) -> `sub_4678C0`: sets the occupied channel's
    /// origin from a mode word and two coordinates; an out-of-range or empty
    /// channel raises 0x10000. Returns 0x4010.
    SetEffectChannelOrigin {
        error_mask: u32,
    },
    /// `sub_47F9A0` (case 337) -> `sub_4679B0`: stores the occupied channel's
    /// playback mode and, for modes 1/3, two coordinates; an out-of-range or
    /// empty channel raises 0x10000. Returns 0x4010.
    SetEffectChannelPlaybackMode {
        error_mask: u32,
    },
    /// `sub_47F1B0` (case 346) -> `sub_467CF0`: stores two dwords on the
    /// occupied channel (record words 618/619); an out-of-range or empty
    /// channel raises 0x10000. Returns 0x400C.
    SetEffectChannelValuePair {
        error_mask: u32,
    },
    /// `sub_47E890` (case 345) -> `sub_466ED0`: on an occupied channel,
    /// applies a named source configuration and reports whether the
    /// channel settled idle (`this+2 == 0 && pending <= 0`) into the
    /// pinned result word; an out-of-range or empty channel raises
    /// 0x10000. Returns 0x4010.
    ConfigureEffectChannelFromName {
        result_field_offset: u32,
        error_mask: u32,
    },
    /// `sub_47FB20` (case 330) -> `sub_467A10`: forwards the occupied
    /// channel's sub-object at record word 631 to `sub_464C00`; an
    /// out-of-range or empty channel raises 0x10000. Returns 0x4004.
    ApplyEffectChannelOperation {
        error_mask: u32,
    },
    /// Native104 cancels a live camera worker and optionally snaps its target.
    CommitScreenParams {
        error_mask: u32,
    },
    /// Native105 preserves PC/operands while active; eligible input may finish it.
    WaitScreenCommit {
        error_mask: u32,
        result_field_offset: u32,
    },
    /// Native106 publishes the actual camera-zero worker activity.
    QueryScreenPending {
        result_field_offset: u32,
    },
    /// `sub_484A70` (case 90) -> `sub_456DF0`: stores the screen object's RGB
    /// fields 1..3 as hundredth-rounded floats. Returns 0x4010.
    ConfigureScreenRgb {
        error_mask: u32,
    },
    /// `sub_484820` (case 91) -> `sub_456C00`: stores the screen object's
    /// rotation field 14. Returns 0x4008.
    ConfigureScreenRotation {
        error_mask: u32,
    },
    /// `sub_4849D0` (case 89) -> `sub_456D80`: stores the screen object's
    /// scale fields 9/10 and their halves at 15/16. Returns 0x400C.
    ConfigureScreenScale {
        error_mask: u32,
    },
    /// `sub_484A20` (case 88) -> `sub_456DC0`: stores the screen object's
    /// offset fields 7/8 and field 6. Returns 0x4010.
    ConfigureScreenOffset {
        error_mask: u32,
    },
    /// `sub_484760`/`sub_4847F0` (cases 92/95): store one value into the
    /// screen object's field 11 or 0. Returns 0x4008.
    ConfigureScreenField {
        error_mask: u32,
        field_offset: u32,
    },
    /// `sub_484980` (case 93) -> `sub_456D60`: stores two values into the
    /// screen object's fields 15/16. Returns 0x400C.
    ConfigureScreenScalePair {
        error_mask: u32,
    },
    /// `sub_484940` (case 94) -> `sub_457CA0`: stores one boolean into the
    /// screen object's field 37. Returns 0x4008.
    ConfigureScreenFlag {
        error_mask: u32,
    },
    /// `sub_4847A0` (case 103) -> `sub_456BE0`: stores a pair into the screen
    /// object's fields 17/18. Returns 0x400C.
    ConfigureScreenPair {
        error_mask: u32,
    },
    /// `sub_484E80` (case 96) -> `sub_456DC0`: stores the offset fields 7/8
    /// and field 6 without a target selector. Returns 0x400C.
    ConfigureScreenOffsetDirect {
        error_mask: u32,
    },
    /// `sub_484E40` (case 97) -> `sub_456D80`: stores the scale fields 9/10
    /// and their halves without a target selector. Returns 0x4008.
    ConfigureScreenScaleDirect {
        error_mask: u32,
    },
    /// `sub_484ED0` (case 98) -> `sub_456DF0`: the RGB fields 1..3 without a
    /// target selector. Returns 0x400C.
    ConfigureScreenRgbDirect {
        error_mask: u32,
    },
    /// `sub_484AC0` (case 99) -> `sub_456C00`: the rotation field 14 without a
    /// target selector. Returns 0x4004.
    ConfigureScreenRotationDirect {
        error_mask: u32,
    },
    /// `sub_484730` (case 100) -> `sub_456690`: the field 11 without a target
    /// selector. Returns 0x4004.
    ConfigureScreenFieldDirect {
        error_mask: u32,
        field_offset: u32,
    },
    /// `sub_484E00` (case 101) -> `sub_456D60`: the fields 15/16 without a
    /// target selector. Returns 0x4008.
    ConfigureScreenScalePairDirect {
        error_mask: u32,
    },
    /// Case 156: the recovered dispatcher body is empty and returns `0x4008`;
    /// it consumes two stack words without reading them. Returns 0x4008.
    NoOpCommand,
    /// Cases 276/277/278 (`sub_488FC0`/`sub_488F80`/`sub_488F40`): write one
    /// field of an effect playback record selected below 12; an out-of-range
    /// or unregistered index raises 0x1000. Returns 0x4008.
    SetEffectPlaybackField {
        error_mask: u32,
        field_offset: u32,
    },
    SetNativeViewSuppression {
        error_mask: u32,
    },
    ConfigureNativeAlternateView {
        result_field_offset: u32,
    },
    ConfigureNativeViewPolicy {
        command_id: u16,
    },
    DetachTextureChildContext,
    /// `sub_488E70` (case 286) -> `sub_462C10`: stores a value dword and two
    /// script strings on an effect playback record's text surface; an
    /// out-of-range or unregistered index raises 0x1000. Returns 0x4010.
    SetEffectTextSurface {
        error_mask: u32,
    },
    /// `sub_47D850` (case 378) -> `sub_445EA0`: stores one effect child
    /// element's visibility; a missing element raises 0x10000. Returns
    /// 0x400C.
    SetEffectElementVisible {
        error_mask: u32,
    },
    /// `sub_47D630` (case 379): answers whether one effect child element
    /// exists; a missing element raises 0x10000. Returns 0x4008.
    QueryEffectElementExists {
        error_mask: u32,
        result_field_offset: u32,
    },
    /// `sub_47D7F0` (case 380) -> `sub_445E80`: stores one effect child
    /// element's size; a missing element raises 0x10000. Returns 0x4010.
    SetEffectElementSize {
        error_mask: u32,
    },
    /// `sub_47E4C0` (386): source rectangle on an existing channel surface.
    SetEffectElementSource {
        error_mask: u32,
    },
    /// `sub_47E540` (387): local destination position on that surface.
    SetEffectElementPosition {
        error_mask: u32,
    },
    /// `sub_47D590` (case 382) -> `sub_445E40`: forwards one effect child
    /// element; a missing element raises 0x10000. Returns 0x4008.
    SetEffectElementAuxiliary {
        error_mask: u32,
    },
    /// `sub_47D5E0` (case 383) -> `sub_445E60`: forwards one effect child
    /// element; a missing element raises 0x10000. Returns 0x4008.
    SetEffectElementAuxiliaryPair {
        error_mask: u32,
    },
    /// `sub_47DC80` (case 463): forwards one effect child element's texture
    /// object to `sub_42BF50`; a missing element raises 0x10000. Returns
    /// 0x4008.
    ApplyEffectElementOperation {
        error_mask: u32,
    },
    /// `sub_47E690`: loads a PB image into the channel's own surface.
    LoadChannelImage {
        channel_table_dword_index: u32,
        max_channel: u32,
        load_error_mask: u32,
        channel_error_mask: u32,
    },
    CreateEffectChannel {
        effect_table_dword_index: u32,
        playback_table_dword_index: u32,
        max_channel: u32,
        max_effect: u32,
        channel_error_mask: u32,
        effect_error_mask: u32,
    },
    /// Case 164: play a paired resource through the alternating audio channels.
    PlayPairedAudio,
    /// Case 256 (`sub_487550`): creates the indexed playback record the
    /// destroy command (257) later frees. The bound index carries the
    /// resolved tagged-name and an extra opaque word into the per-index
    /// tables at `+0xBB8`/`+0xBE8`; the headless record keeps only the
    /// occupancy.
    CreateEffectPlayback {
        max_index: u32,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
    },
    /// Case 320 (`sub_47EA90`): rebuilds table 742's channel and table 750's
    /// text owner in the current stage coordinate space.
    CreateStageEffectChannel {
        max_channel: u32,
        max_index: u32,
        channel_error_mask: u32,
        index_error_mask: u32,
    },
    /// Case 258 (`sub_489160`): forwards the tagged name and the opaque
    /// word to the occupied playback record at the bound index; an
    /// unoccupied or out-of-range index only raises the playback error
    /// mask. The headless record has no engine work to schedule.
    ForwardEffectPlaybackSetup {
        max_index: u32,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
    },
    /// Case 259 (`sub_4878C0`): the case-258 shape with three more opaque
    /// operand words; the occupied record at the bound index consumes all
    /// of them, an unoccupied or out-of-range index only raises the
    /// playback error mask.
    ConfigureEffectPlaybackSlots {
        max_index: u32,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
    },
    /// Stores one opaque word in a CMVS interpreter-owned field. The field
    /// offset is part of the version-pinned contract; its game-level meaning
    /// remains deliberately unnamed until a consumer is recovered.
    StoreInterpreterWord {
        field_offset: u32,
    },
    /// Stores the two top stack words into a CMVS interpreter-owned field
    /// pair (`sub_474626`: top word to the first offset, next word to the
    /// second; returns 0x4008).
    StoreInterpreterWordPair {
        top_field_offset: u32,
        second_field_offset: u32,
    },
    /// Ordinary streaming audio handlers, consumed by the session PCM worker.
    NativeAudio {
        command_id: u16,
    },
    /// Writes a proven immediate value to a CMVS interpreter-owned field.
    StoreInterpreterConstant {
        field_offset: u32,
        value: u32,
    },
    /// Publishes the input manager's confirmed selection id
    /// (`sub_45CBB0`, field 347) into an interpreter-owned field
    /// (`sub_479C90`, case 846). The port publishes the session's
    /// armed load-confirm selection when one is armed and zero
    /// otherwise, the form every save-verified flow was proven
    /// against.
    PublishInputSelection {
        field_offset: u32,
    },
    /// Zeroes one word of a bounded interpreter-owned word table selected by
    /// the stack-top index (`sub_478B50` indexes the 10-word family starting
    /// at the version-pinned base offset); out-of-range indices block
    /// dispatch because the original writes outside the recovered fields.
    ClearInterpreterTableWord {
        table_base_offset: u32,
        table_word_count: u32,
    },
    /// Copies one word of the bounded interpreter-owned word table selected
    /// by the stack-top index into a version-pinned destination field
    /// (`sub_478B80` reads the zero-initialized `this+3307` family); indices
    /// outside the family block dispatch.
    LoadInterpreterTableWord {
        table_base_offset: u32,
        table_word_count: u32,
        field_offset: u32,
    },
    /// Writes all-ones into the first word of one 28-byte record of the
    /// bounded interpreter record table (`sub_47CC80` indexes the 64-record
    /// family at byte offset 13280); indices above 63 are silently ignored.
    MarkInterpreterRecordUnused {
        record_table_base_offset: u32,
        record_count: u32,
        record_stride_bytes: u32,
    },
    /// The recovered case-747 flag round trip (`sub_479400`): a non-zero
    /// selector stores the second word as the boolean flag at the
    /// version-pinned field; a zero selector copies the stored flag back
    /// into the expression-result field.
    InterpreterFlagRoundTrip {
        flag_field_offset: u32,
        result_field_offset: u32,
    },
    /// Resets one interpreter-owned string buffer to a recovered interpreter
    /// prefix field and appends the resolved private string reference. The
    /// original helper copies the prefix, not an empty string, so the buffer
    /// content is retained as an ordered, payload-free segment sequence.
    /// `ensure_directories` marks the case-22 path that additionally walks
    /// the assembled path and creates missing directories.
    StorePrefixedInterpreterString {
        buffer_offset: u16,
        prefix_field_offset: u16,
        ensure_directories: bool,
    },
    /// Stores one opaque word through a recovered CMVS process-global setter.
    /// The absolute address is a CMVS 3.90 fingerprint, not a portable symbol.
    StoreProcessGlobalWord {
        address: u32,
    },
    /// Stores a canonical zero/one value through a recovered CMVS
    /// process-global boolean setter.
    StoreProcessGlobalBoolean {
        address: u32,
    },
    /// Sets or clears a bounded range in the recovered CMVS process flag
    /// bitmap. The bit ids remain opaque until their consumers are recovered.
    MutateProcessFlagRange,
    /// Writes one opaque value across a bounded CMVS process-indexed range.
    StoreProcessIndexedRange,
    /// Writes one raw f32 bit-pattern across a bounded CMVS process range.
    StoreProcessFloatRange,
    /// Copies one resolved private string reference to a bounded CMVS process
    /// string-slot range.
    StoreProcessStringRange,
    /// Writes a proven immediate value to a field on an interpreter-owned
    /// component selected by its parent object offset.
    StoreComponentConstant {
        component_offset: u16,
        field_offset: u16,
        value: u32,
    },
    /// Copies the source path into one independent native search table.
    /// This configures resource lookup; it does not start audio playback.
    ConfigureResourceSearch {
        bank: CmvsResourceSearchBank,
    },
    /// Case 356 (`48A6C0`): request real fullscreen mode and suspend.
    SetNativeFullscreenSuspend,
    /// Begins the recovered CMVS resource-channel setup boundary for one of
    /// the statically proven channel banks. The bank fingerprint is the
    /// original setup-handler address; slot counts come from its bounds
    /// check.
    StartResourceChannel {
        bank: CmvsResourceChannelBank,
    },
    /// Creates one opaque object in the bounded interpreter slot table,
    /// replacing any previous occupant. Out-of-range slots only update the
    /// recovered interpreter error-flag word, mirroring the original handler.
    CreateSlotObject {
        table_byte_offset: u16,
    },
    /// Creates one opaque object in a fixed singleton slot of the bounded
    /// interpreter slot table (the case-544 filter-graph owner replaces any
    /// previous occupant) and stores a version-pinned interpreter word, all
    /// without inspecting the four consumed stack words.
    CreateSingletonSlotObject {
        table_byte_offset: u16,
        slot: u8,
        field_offset: u32,
        value: u32,
    },
    /// The recovered case-548 filter-graph control. The handler clears the
    /// version-pinned flag word first; a missing singleton or a faulted
    /// remaining-length check (`sub_458E50`) then returns 0x4004, a healthy
    /// zero control returns 0xA000 without a pop, and the non-zero control
    /// playback start (0xC004) needs the unrecovered media graph.
    RunFilterGraphControl {
        table_byte_offset: u16,
        slot: u8,
        field_offset: u32,
    },
    /// Destroys the occupant of a fixed singleton slot of the bounded
    /// interpreter slot table (`sub_47A5F0` frees the case-544 filter-graph
    /// owner when present) without touching the stack.
    DestroySingletonSlotObject {
        table_byte_offset: u16,
        slot: u8,
    },
    /// Replaces the occupant of one six-slot filter-chain table
    /// (`sub_485FD0` bounds the bank to six and the channel to 256,
    /// raising the 0x100000 error mask otherwise); the construction words
    /// stay opaque and only slot occupancy is observable.
    ReplaceFilterChainSlot {
        table_byte_offset: u16,
        max_bank: u32,
        max_channel_id: u32,
        error_flag_field_offset: u16,
        error_flag_mask: u32,
    },
    /// Destroys one of the six recovered interactive-chain banks. The
    /// original frees the owned chain object, clears the table entry and
    /// raises the same error mask as construction when the bank is outside
    /// the recovered range.
    DestroyFilterChainSlot {
        table_byte_offset: u16,
        max_bank: u8,
        error_flag_field_offset: u16,
        error_flag_mask: u32,
    },
    /// Queries one six-slot filter-chain table entry (`sub_486BC0`): a
    /// missing bank raises the 0x100000 error mask, while a live bank
    /// appends one opaque queue node through `sub_468560` and stores one
    /// into the expression-result field; the query word stays unobserved.
    QueryFilterChainSlot {
        table_byte_offset: u16,
        max_bank: u32,
        error_flag_field_offset: u16,
        error_flag_mask: u32,
        result_field_offset: u32,
    },
    /// Updates four opaque words in the keyed record of one live filter-chain
    /// bank. A missing key is a no-op; a missing or out-of-range bank raises
    /// the recovered error mask.
    UpdateFilterChainRecord {
        table_byte_offset: u16,
        max_bank: u8,
        error_flag_field_offset: u16,
        error_flag_mask: u32,
    },
    /// Writes one recovered 16-byte parameter block in a keyed filter-chain
    /// record. The process-global backend selector controls the original
    /// block layout; selectors outside its recovered range are no-ops.
    UpdateFilterChainParameterBlock {
        table_byte_offset: u16,
        max_bank: u8,
        backend_mode_address: u32,
        error_flag_field_offset: u16,
        error_flag_mask: u32,
    },
    /// Applies every queued record of one live filter-chain bank through the
    /// original backend path (`sub_486D30` -> `sub_4686A0`). The command has
    /// no expression result; the deterministic revision records the apply
    /// boundary while the still-opaque backend fields remain in VM state.
    ApplyFilterChainRecords {
        table_byte_offset: u16,
        max_bank: u8,
        error_flag_field_offset: u16,
        error_flag_mask: u32,
    },
    /// Polls one recovered six-bank interactive record chain. The original
    /// handler updates the active record on directional input and returns
    /// the active record id on confirm, `-2` on cancel, or `-1` while idle.
    PollFilterChainSelection {
        table_byte_offset: u16,
        max_bank: u8,
        error_flag_field_offset: u16,
        error_flag_mask: u32,
        result_field_offset: u32,
    },
    /// Reads the active record id from one recovered interactive chain. A
    /// live bank without an active record yields `-1`.
    ReadActiveFilterChainSelection {
        table_byte_offset: u16,
        max_bank: u8,
        error_flag_field_offset: u16,
        error_flag_mask: u32,
        result_field_offset: u32,
    },
    /// Destroys the opaque object held in one bounded interpreter slot.
    DestroySlotObject {
        table_byte_offset: u16,
    },
    /// Moves the opaque object from the second bounded slot into the first,
    /// clearing the source slot.
    MoveSlotObject {
        table_byte_offset: u16,
    },
    /// Sets an owned native window caption (case 352).
    RequestWindowCaption,
    /// Loads a file icon for WM_SETICON/ICON_SMALL (case 143).
    RequestWindowIcon {
        prefix_field_offset: u16,
    },
    /// Copies the resolved stack string into one bounded interpreter buffer,
    /// raising the recovered error-flag mask when the resolved text exceeds
    /// the byte bound, mirroring `sub_48A670`.
    StoreBoundedInterpreterString {
        buffer_offset: u16,
        max_text_bytes: u16,
        error_flag_field_offset: u16,
        error_flag_mask: u32,
    },
    /// Builds the case-353 caption: the interpreter prefix field, a
    /// conditional two-space gap selected by the unrecovered host gate and
    /// the resolved stack string; stores the resolved reference into the
    /// named buffer and hands the segments to the host, mirroring
    /// `sub_48A710`.
    BuildPrefixedWindowCaption {
        prefix_field_offset: u16,
        buffer_offset: u16,
    },
    /// Writes one stack word to a field on an interpreter-referenced
    /// component selected by its parent field offset.
    StoreComponentWord {
        component_field_offset: u16,
        field_offset: u16,
    },
    /// Polls one guarded field on an interpreter-referenced component
    /// (`sub_48C290`: `[[this+parent]+field]`) and publishes its boolean
    /// into the version-pinned result word; the writer side is engine
    /// message 0x8006 (`sub_48CDD0`).
    QueryComponentWordBooleanToResult {
        component_field_offset: u16,
        field_offset: u16,
        result_field_offset: u32,
    },
    /// Case 569 (`sub_489B30`): publishes the raw guarded field
    /// (`[[this+parent]+field]` through `sub_48C3B0`) into the version-
    /// pinned result word without booleanizing it.
    QueryComponentWordToResult {
        component_field_offset: u16,
        field_offset: u16,
        result_field_offset: u32,
    },
    /// Stores the stack pair into two interpreter fields and derives a
    /// non-zero pair flag, all at version-pinned byte offsets.
    StorePresentationFrameFields {
        top_field_offset: u16,
        second_field_offset: u16,
        active_field_offset: u16,
    },
    /// Scans the native save menu's bounded slot catalog (case 691).
    ScanNativeSaveSlots,
    /// Loads the named script into a nested frame through the host and
    /// transfers dispatch there, mirroring `sub_478080`. The frame index
    /// bound is enforced before any state changes.
    CallScript,
    /// Loads the named script as the new root frame-0 script, resets the
    /// interpreter stack, frame table and current value, and transfers
    /// dispatch to the new script's entry PC, mirroring `sub_4781B0`.
    ReloadRootScript,
    /// Tears down every engine-side object table (the 256-slot channel
    /// table, filter chains, texture parents and children, native text,
    /// effect channels and playback), clears the record-28 fallback words
    /// and reloads `main.ps3` as the root frame-0 script, mirroring the
    /// `sub_477AC0` teardown behind dispatcher case 3.
    ResetEngineToMain,
    /// Reads a point and camera handle off the stack (`sub_484890`) and
    /// stores the camera-projected coordinates, truncated to integers, into
    /// three consecutive interpreter words. The kind-3 camera transform of
    /// `sub_456C40` needs the camera object graph, which is not recovered;
    /// the identity branch (coordinates pass through) is the faithful
    /// observable subset until then.
    ProjectPointToScreenWords {
        result_base: u32,
    },
    /// `sub_47E9E0` (case 343): the wait twin of case 345 — applies the
    /// named source configuration to an occupied effect channel and ends
    /// the frame once the channel settles (`0xC01C`). The recovered channel
    /// objects carry no pending queue, so the headless model settles
    /// immediately; dead or out-of-range channels raise the shared error
    /// flag instead.
    PrepareEffectChannelFromName {
        error_mask: u32,
    },
    /// `47F7D0 -> 467510`: automatic text/voice wait, then 0x400C.
    WaitChannelAutoAdvance,
    /// `47A370 -> 425B60`: replaces the parent image, retaining the old
    /// surface as its transition owner. Reports the real loader result.
    /// Returns `0x4010` (pop 16).
    ReloadTextureParentImage {
        result_field_offset: u32,
        table_byte_offset: u16,
    },
    /// `sub_486E90` (case 252) -> `sub_418D70`: native type-before-width
    /// formatting of one CP932 format and one stack word into the storage the
    /// first tagged word resolves to (frame pool offset or process string
    /// slot), NUL-terminated. Returns `0x400C` (pop 12).
    FormatStringIntoSlot,
    /// `sub_427E80` behind dispatcher case 297: serializes the system
    /// state (flags, CG/BGM tables, title slots, both native lists) into
    /// the ciphered `save/system.dat` container. Returns `0x4000`.
    SaveSystemState,
    /// Writes one bounded 28-byte record into the recovered interpreter
    /// record table: a zero flag, the current frame index, four stack
    /// words and a clock word. Out-of-range indices leave the table
    /// untouched, matching `sub_47CBE0`.
    StoreInterpreterTimestampedRecord {
        table_base_offset: u16,
        record_stride_bytes: u8,
        max_index: u8,
    },
    /// Pops the stack-top coroutine label and transfers dispatch to the
    /// (frame, PC) pair the 28-byte interpreter record table registered for
    /// it, mirroring `sub_47CB00`. Before the transfer the advanced PC, the
    /// current frame index and the frame-counter field are pushed as the
    /// nested-call return record and the frame-counter field clears, matching
    /// the `sub_478080` push order. The original masks the label to the
    /// 64-record bound; a record without a registered PC (missing or all-
    /// ones) blocks instead of emulating the original's unchecked jump.
    ResumeInterpreterCoroutineRecord {
        record_table_base_offset: u16,
        record_stride_bytes: u8,
        max_index_mask: u8,
    },
    /// 549/47AFF0 creates the native 459BE0 screen-shake owner.
    CreateScreenShake {
        handle_field_offset: u16,
    },
    /// 550/459CC0 returns remaining periods, or all ones for an infinite/missing owner.
    QueryScreenShakeRemaining {
        result_field_offset: u32,
    },
    /// 551 pauses the owner; paused shaking yields zero offsets.
    SetScreenShakePaused,
    /// 552 waits without popping while an owner exists, or cancels on false.
    WaitOrDestroyScreenShake {
        result_field_offset: u32,
    },
    /// Case 553 (`sub_47B120`): destroys a present owner. No stack pop.
    DestroyScreenShake,
    /// 524 queries whether a live owner is paused.
    QueryScreenShakePaused {
        result_field_offset: u32,
    },
    /// Case 294 (`sub_48A570`): the version-pinned result pair (system
    /// registers 1 and 2) receives the render object's inner display-mode
    /// fields 6 and 7 through `sub_432F70`/`sub_432F30`. The host rebinds
    /// these configured native dimensions separately from base coordinates
    /// and physical raster size. Unbound queries fail. No stack pop.
    QueryRendererDisplayMode {
        renderer_field_offset: u32,
        result_field_offset: u32,
    },
    /// Case 200 (`sub_480280`): writes the session pointer position into the
    /// version-pinned result pair (system registers 1/2) through
    /// `sub_45CF40`, which reads the input-manager object's fields 180/181.
    /// No stack pop.
    QueryPointerPosition {
        x_field_offset: u32,
        y_field_offset: u32,
    },
    /// Case 202 (`sub_480060`): inclusive-bounds hit test of the session
    /// pointer against the stack-provided rectangle; the boolean lands in
    /// the version-pinned result field. Returns 0x4010.
    HitTestPointerRect {
        result_field_offset: u32,
    },
    /// Case 203 (`sub_4800F0`): GDI region hit test (`PtInRegion`) of the
    /// session pointer against rectangle, ellipse, round-rectangle or WINDING
    /// polygon regions. Returns 0x401C.
    HitTestPointerRegion {
        result_field_offset: u32,
    },
    /// Case 333 (`sub_47F270`): reads one occupied effect channel's
    /// transient activity flag (the object's field +8, which case 334
    /// resets when the playback flag changes) into the version-pinned
    /// result field. An out-of-range or empty channel only raises the
    /// channel error mask. Returns 0x4004.
    QueryEffectChannelActivity {
        max_channel: u32,
        effect_table_dword_index: u32,
        error_mask: u32,
        result_field_offset: u32,
    },
    /// Case 718 (`sub_472F10` case body): copies one version-pinned
    /// interpreter word directly into the result field without touching the
    /// stack. Returns 0x4000.
    LoadInterpreterWordToResult {
        source_field_offset: u32,
        result_field_offset: u32,
    },
    /// Cases 729..733: mode zero reads the master/BGM/SFX/voice/system
    /// gain into 81220; any nonzero mode stores a signed clamp to 0..256
    /// and synchronously applies that setting to live audio. Pops eight bytes.
    NativeAudioGainSetting {
        command_id: u16,
    },
    /// Case 750 (`sub_472F10` case body): writes one when the version-pinned
    /// interpreter word at byte offset 1596 is non-zero into the result
    /// field, and zero otherwise. Returns 0x4000.
    LoadInterpreterWordBooleanToResult {
        source_field_offset: u32,
        result_field_offset: u32,
    },
    /// Cases 424/426/428/430/432 (`sub_47A0B0`/`sub_479D70`/`sub_479E30`/
    /// `sub_47A070`/`sub_479FB0`): write the two booleans of one scene-object
    /// layer group into the version-pinned result pair at byte offsets 81220
    /// and 81236. The group is addressed by its base dword index; the reader
    /// touches `base` and `base + 1`. Returns 0x4000.
    QuerySceneLayer {
        base_word: u32,
        result_field_offset: u32,
        aux_field_offset: u32,
    },
    /// Cases 425/427/429/431/433 (`sub_4616E0`/`sub_4612B0`/`sub_461310`/
    /// `sub_4616C0`/`sub_4613D0`): clear the base and `base + 2` words of one
    /// scene-object layer group, leaving `base + 1` untouched. No stack
    /// operands; returns 0x4000.
    ClearSceneLayer {
        base_word: u32,
    },
    /// Case 166 (`sub_489630`): lays out one tagged message string through
    /// the window engine's text measurement (`sub_48DB70`) with a scale
    /// selector and a bold flag. The selector must stay below four, raising
    /// `error_flag_mask` at `error_flag_field_offset` otherwise. The
    /// headless page renderer lays the message text out itself, so the
    /// recovered handler reports the layout as applied and leaves the
    /// measured extent at zero. Returns 0x4010.
    LayoutMessageText {
        max_selector: u32,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
        result_field_offset: u32,
        extent_field_offset: u32,
    },
    /// Case 645 (`4243B0`): publishes the queued timeline record count.
    ReadEffectTimelineCount {
        result_field_offset: u32,
    },
    /// Case 646 (`sub_47C240` -> `sub_4246C0`): appends a timed delay step
    /// (type 1, duration from the stack) to the sixteen-record effect
    /// timeline. Returns `0x4004`.
    PushEffectTimelineDelay,
    /// Case 648 (`4245A0`): queues a parent-image replacement (type 3,
    /// mode 0), including its resource name and transition duration.
    PushEffectTimelineChannel,
    /// Case651 queues channel-zero child image replacement (native type5).
    PushEffectTimelineElement,
    /// Case 772 (`sub_426670`): sets interpreter dword 2408 to one.
    /// Returns `0x4000`.
    SetPresentAllFlag {
        field_offset: u32,
    },
    /// Case 774 (`sub_485E70`): sysreg0 becomes `this[2411] != 0`, the
    /// present-complete flag. Returns `0x4000`.
    ReadPresentCompleteFlag {
        result_field_offset: u32,
        complete_field_offset: u32,
    },
    /// Case 60 (`sub_4843E0`): hit query on a table-1924 slot object
    /// guarded by a negative second word; consumes the present-complete
    /// word and stores the test result in sysreg0. Returns `0x4008`.
    QueryPresentSlotHit {
        result_field_offset: u32,
    },
    /// Case 773 (`sub_426680`): clears the four-word present block
    /// (dwords 2408..2411). Returns `0x4000`.
    ClearPresentBlock,
    /// Case 559 (`sub_47B1E0`): rebuilds the native graphics context; the
    /// headless pipeline keeps the sysreg0 success store. Returns
    /// `0x4000`.
    RebuildGraphicsContext {
        result_field_offset: u32,
    },
    /// Case 563 (`sub_47B4C0`): a skippable present wait — zero mode waits
    /// one frame; non-zero mode breaks out with sysreg0 = 1 when the skip
    /// latch family fires. Returns `0xA000`/`0xC004`.
    PresentSkippableWait {
        result_field_offset: u32,
    },
    /// Case 560/561 (`sub_47B470`/`sub_47B400`): destroy the graphics
    /// context (560 only while the render state allows). Returns
    /// `0x4000`.
    ReleaseGraphicsContext {
        require_render_state: bool,
    },
    /// Case 562 (`sub_47B440`): sysreg0 becomes the render state, or -1
    /// without a context. Returns `0x4000`.
    QueryGraphicsContext {
        result_field_offset: u32,
    },
    /// Case 644 (`47C040`): skips pending delays, completes image
    /// replacements and clears the five timeline header words.
    RunEffectTimeline,
    /// `sub_48A3A0` (case 183): sysreg0 becomes the slot-status word
    /// for selectors 0-5 (canonical zero headless); above five raises
    /// 0x200. Returns 0x4004.
    /// `sub_483530` (case 76): stores the value word through the
    /// 481-table child; the slot gate raises 0x10. Returns 0x400C.
    /// `sub_489080` (case 282): stores the value word into the
    /// 750-table playback record; slots at or above twelve raise 0x1000.
    /// Returns 0x4008.
    StorePlaybackRecordWord,
    StoreTextureChildByIndex,
    /// `sub_480A80` (case680) moves a481-table CImage into a737 shard worker.
    /// The source image and its existing subtree survive. Returns0x4010.
    RebuildSceneLayerChild,
    /// `sub_481590` (case 672): destroys the scene-layer worker at the
    /// 737-slot (<= 3). Returns 0x4004.
    DestroySceneLayerWorker737,
    /// `sub_47E720` (case 369): destroys the hit element on the occupied
    /// channel; the gate raises 0x10000. Returns 0x4004.
    DestroyEffectHitElement,
    /// Case 647 queues a camera-zero transition (native type 4).
    PushEffectTimelineCamera,
    /// Cases 649/650 queue native type-3 mode-1/2 transforms.
    PushEffectTimelineTransform {
        mode: u8,
    },
    /// Case 420 (`sub_479AE0`): reads the auxiliary (skip) button group
    /// latches (`sub_45CC00`). The pump maps no physical key to this
    /// group yet, so both fields read zero. Returns `0x4000`.
    ReadAuxLatches {
        held_field_offset: u32,
        edge_field_offset: u32,
    },
    /// Case 418 (`sub_479A70`): reads the input manager's cancel-group
    /// latches (`sub_45CBD0`) into the two result words. The recovered
    /// pump tracks only the held state, so both fields receive it.
    /// Returns `0x4000`.
    ReadCancelLatches {
        held_field_offset: u32,
        edge_field_offset: u32,
    },
    /// Case 310 (`47CFA0/468130`) owns a new native history entry.
    AppendNativeMessageHistory,
    /// Case 303 clears native history nodes but retains their capacity.
    ClearNativeMessageHistory,
    /// Case 640 (`424790`): clears all five timeline header words.
    ResetEffectTimeline,
    /// Cases 642/643 (`424AC0`/`424B10`): start and stop the timeline.
    StartEffectTimeline,
    StopEffectTimeline,
    /// Case 641 (`424B00`): publishes the active cursor, or -1.
    ReadEffectTimelineCursor {
        result_field_offset: u32,
    },
    /// Case 720 (`sub_479860`): a non-zero selector applies the window
    /// cursor style `abs(value) % 3` engine-side; a zero selector
    /// publishes the style setting word into the result field.
    /// Returns 0x4008.
    StoreWindowStyleSetting {
        setting_field_offset: u16,
        result_field_offset: u32,
    },
    /// Case 690 (`sub_485D10`): builds the save snapshot blob through
    /// `sub_4712F0` outside the interpreter state. No recovered interpreter
    /// word changes; the write itself stays native. Returns `0x4000`.
    BuildSaveSnapshot,
    /// `47F6E0 -> 467060`: confirmation or configured automatic text/voice
    /// wait. Pending returns 0xA000, completion 0x400C.
    WaitChannelAdvance,
    /// `sub_47E910`: display channel text; 0xA000 while glyphs are pending,
    /// 0xC00C when complete. The second operand is a read-bitmap id.
    DisplayChannelText,
    /// Case 417 (inline `sub_45CB80` on the input manager): consumes the
    /// advance-group latch edges (release 271 and press 273). The command
    /// takes no stack operands and returns `0x4000`.
    ConsumeAdvanceLatch,
    /// Case 860 (`sub_479C60`): stores whether the input manager's
    /// advance-group press latch (`sub_45CCD0`, manager dword 273) reads
    /// non-zero. Takes no stack operands and returns `0x4000`.
    StorePressLatch {
        result_field_offset: u32,
    },
    /// Case 72 (`sub_4837F0`): rebinds one texture container child's audio
    /// cue. The recovered subset validates the container liveness and the
    /// child bound (`sub_444EF0`/`sub_446B10` stay outside it) and raises
    /// the matching error masks. Returns 0x400c.
    ConfigureTextureChildAudio {
        texture_table_base_offset: u32,
        max_parent_slot: u32,
        max_child_id: u32,
        container_error_flag_field_offset: u32,
        container_error_flag_mask: u32,
        child_error_flag_field_offset: u32,
        child_error_flag_mask: u32,
    },
    /// Case 500 (`sub_4823E0`): draws one texture container child onto the
    /// parent surface. The handler validates the container slot (`< 0x100`
    /// and occupied, raising `parent_error_mask`) and the child selector
    /// (`< 0x400`, raising `child_error_mask`) before the engine draw chain
    /// `sub_444EF0`/`sub_445340`/`sub_42BC90` runs; the headless renderer
    /// composes children from the recovered state, so the draw itself has no
    /// additional interpreter-visible effect. Returns 0x4008.
    DrawTextureChild {
        texture_table_base_offset: u32,
        max_parent_slot: u32,
        max_child_id: u32,
        error_flag_field_offset: u32,
        parent_error_mask: u32,
        child_error_mask: u32,
    },
    /// Case 494 (`sub_4829C0`): publishes one texture container child's load
    /// state into a result field. The handler validates the container slot
    /// (`< 0x100` and occupied, raising `container_error_mask`) and the child
    /// selector (`< 0x400`, raising `child_error_mask`) before reading
    /// `sub_42C270(sub_445340(child))`: a negative value means the child
    /// loader has not produced its sub-object yet. The headless texture
    /// pipeline decodes synchronously, so the loader reports the idle state
    /// zero. Returns 0x4008.
    QueryTextureChildState {
        texture_table_base_offset: u32,
        max_parent_slot: u32,
        max_child_id: u32,
        container_error_flag_field_offset: u32,
        container_error_mask: u32,
        child_error_flag_field_offset: u32,
        child_error_mask: u32,
        result_field_offset: u32,
    },
    /// Case 493 (`sub_482B00`): destroys one texture container child's load
    /// state (`sub_42C2A0` joins and frees the loader sub-object). The
    /// handler validates the container slot (`< 0x100` and occupied,
    /// raising `container_error_mask`) and the child selector (`< 0x400`,
    /// raising `child_error_mask`). The headless texture pipeline decodes
    /// synchronously and holds no loader object, so the teardown has no
    /// further effect. Returns 0x4008.
    ReleaseTextureChildState {
        texture_table_base_offset: u32,
        max_parent_slot: u32,
        max_child_id: u32,
        container_error_flag_field_offset: u32,
        container_error_mask: u32,
        child_error_flag_field_offset: u32,
        child_error_mask: u32,
    },
    /// Case 397 (`sub_47E300`): copies one effect quad sub-object's fields    /// (indices 17, 18, 19, 8, 9, 10) into six version-pinned result
    /// fields. The headless effect graph holds no animated state, so the
    /// recovered subset writes zeros. An out-of-range/empty channel raises
    /// `empty_error_mask`; a missing sub-object raises
    /// `missing_error_mask`. Returns 0x4008.
    QueryEffectQuadState {
        effect_table_dword_index: u32,
        empty_error_mask: u32,
        missing_error_mask: u32,
        result0_field_offset: u32,
        result1_field_offset: u32,
        result2_field_offset: u32,
        result3_field_offset: u32,
        result8_field_offset: u32,
        result9_field_offset: u32,
    },
    /// Case 406 (`sub_47F330`): selects/activates one quad slot on an
    /// occupied effect channel with a value word. The headless effect graph
    /// keeps only occupancy, so the value is not retained. Returns 0x400C.
    ActivateEffectQuad {
        error_mask: u32,
    },
    /// Case 695 (`sub_485970`): scans the save-slot range
    /// `[start, start + count)` for the newest `saveNN.dat` and writes its
    /// slot index, or -1 when the range is invalid. The headless host
    /// exposes no writable save files, so the result is always -1. Returns
    /// 0x4008.
    QueryNewestSaveSlot {
        result_field_offset: u32,
    },
    /// Applies the recovered settings-object reset and stores one boolean
    /// setting at a version-pinned field offset.
    StoreSystemSettingBoolean {
        setting_field_offset: u16,
    },
    /// Applies the recovered settings-object reset and stores one opaque
    /// stack word at a version-pinned field offset, mirroring `sub_45C470`.
    StoreSettingsWordReset {
        setting_field_offset: u16,
    },
    /// The recovered case-212 handler (`sub_484F60`) reduces the next
    /// pseudo-random draw modulo the stack-top bound and stores the result
    /// at a version-pinned interpreter field. The draw comes from the
    /// deterministic PRNG state retained in the snapshot; a zero bound is
    /// blocking because the original's divide would fault.
    StoreRandomModulo {
        target_field_offset: u32,
    },
    /// The recovered case-176 handler (`sub_48A110`) registers one resource
    /// channel slot record: out-of-range slots only raise the error-flag
    /// mask, otherwise the name/volume/enabled/loop words are copied into
    /// the fixed 52-byte record table. The playback gate (`sub_48DB70`)
    /// stays unrecovered and blocks when its gate field is non-zero.
    RegisterResourceChannelSlot {
        max_slot: u8,
        table_base_offset: u16,
        record_stride_bytes: u8,
        gate_field_offset: u16,
        error_flag_field_offset: u16,
        error_flag_mask: u32,
    },
    /// The recovered case-34 handler (`sub_4805E0`) selects the top-level
    /// texture container with the stack-top word and recreates its child
    /// texture selected by the second word. A missing/out-of-range parent
    /// raises the parent error mask; child ids at or above 1024 are ignored
    /// by `sub_4442C0`, matching the original no-op.
    ResetTextureChild {
        texture_table_base_offset: u32,
        max_parent_slot: u8,
        max_child_id: u16,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
    },
    /// 481D30/case 38 writes native root/child visibility; hidden roots hide their subtree.
    SetTextureVisibility,
    /// Loads a tagged PB resource into a live top-level texture container.
    /// Handler `sub_483D70` stores the synchronous loader result in the
    /// interpreter expression-result word and returns 0x4008.
    LoadTextureParentResource {
        texture_table_base_offset: u32,
        max_parent_slot: u8,
        result_field_offset: u32,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
    },
    /// Case 410 (`sub_45C5A0`): resets the presentation manager's internal
    /// counters; no observable headless state.
    ResetPresentationBuffers,
    /// 46/4804C0 ->445E60 clears the selected image header's shake bit0x100.
    DisableTextureShake,
    /// 480620/case 35 destroys one child; out-of-range child ids are ignored.
    /// The parent loader and other children retain their ownership.
    DestroyTextureChild {
        texture_table_byte_offset: u16,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
    },
    /// `sub_478EC0` (case 721): stores a config boolean or reads the skip
    /// flag. Returns 0x4008.
    StoreSystemConfigBoolean {
        config_field_offset: u32,
        skip_flag_field_offset: u32,
        result_field_offset: u32,
    },
    /// `sub_480560` (case 39): stores whether the selected script slot is
    /// occupied. Returns 0x4008.
    QueryScriptSlotOccupancy {
        slot_table_dword_index: u32,
        max_slot: u32,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
        result_field_offset: u32,
    },
    /// `sub_47AF00` (case 555): commits the presented texture for the
    /// frame; the wait or show outcome ends the dispatch for the frame.
    CommitTexturePresentation {
        result_field_offset: u32,
        present_object_field_offset: u32,
    },
    /// `sub_47ACA0` / `sub_47AA50`: start a mask transition over an
    /// existing parent, or over the newly cleared parent zero.
    StartTexturePresentation {
        clear_background: bool,
    },
    /// `sub_47AED0`: remaining transition milliseconds, or -1 if absent.
    QueryTexturePresentation,
    /// `sub_47AE00`: finish immediately, destroying a hidden parent.
    FinishTexturePresentation,
    /// Cases 64/65 create or free the basic surface, retaining its image provider.
    /// Handlers 482F20/482F60 consume one parent id and return 0x4004.
    SetTextureParentSurface {
        initialized: bool,
        texture_table_base_offset: u32,
        max_parent_slot: u8,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
    },
    /// The recovered case-47 handler (`sub_4803D0`) tears down every live
    /// entry of the 256-slot texture channel table without reading the stack.
    ClearTextureChannelTable {
        channel_table_base_offset: u32,
    },
    /// The recovered case-248 handler (`sub_47D220`) resolves the stack-top
    /// tagged string and stores its `lstrlenA` byte length into one
    /// interpreter word. The length comes from the frame's payload-free
    /// string-length table injected at script load.
    StoreStringLength {
        target_field_offset: u32,
    },
    /// Case 243 (`sub_47D2B0`): the case-`StoreStringLength` shape with a
    /// destination tag, a second source tag and two opaque words (the
    /// engine copies the source text through the DBCS lead-byte walk and
    /// pins the display-unit count into the result word). The headless
    /// frame table already tracks pool lengths, so only the count store is
    /// observable.
    CopyTaggedStringLengthToResult {
        result_field_offset: u32,
    },
    /// The recovered case-56 handler (`sub_480700`) resolves the third stack
    /// word as a PB image and loads it into the child selected by the first
    /// two words of one live top-level texture container.
    LoadTextureChildResource {
        texture_table_base_offset: u32,
        max_parent_slot: u8,
        max_child_id: u16,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
    },
    /// The recovered case-80 handler (`sub_480660`) creates the renderable
    /// surface state of one existing texture child.
    InitializeTextureChildSurface {
        texture_table_base_offset: u32,
        max_parent_slot: u8,
        max_child_id: u16,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
    },
    /// 483A80 -> 446BF0 -> 42EA20 stores raw IEEE754 X/Y scale words.
    ConfigureTextureScale,
    ConfigureTextureBasicKind {
        command_id: u16,
    },
    ConfigureTextureBlend,
    ConfigureTextureFloatPosition,
    ConfigureTextureRotation,
    ConfigureTextureProjectionField {
        field_index: u8,
    },
    /// Case 67 (`482EA0` -> `4443F0` -> `42C300`): resets the basic
    /// surface rectangle to (0, 0, width, height) and sets its opaque color.
    ConfigureTextureSolidSurface {
        texture_table_base_offset: u32,
        max_parent_slot: u8,
        error_flag_field_offset: u32,
        parent_error_mask: u32,
    },
    /// Case 68 (`446B30` -> `42E950`): source rectangle (x, y, width, height).
    /// A signed negative child selector addresses the parent's basic surface.
    ConfigureTextureChildRect {
        texture_table_base_offset: u32,
        max_parent_slot: u8,
        max_child_id: u16,
        error_flag_field_offset: u32,
        parent_error_mask: u32,
        child_error_mask: u32,
    },
    /// Case 70 (`446AD0` -> `42E870`): signed-low-word geometry pivot (DWORD6/7).
    ConfigureTextureChildPosition {
        texture_table_base_offset: u32,
        max_parent_slot: u8,
        max_child_id: u16,
        error_flag_field_offset: u32,
        parent_error_mask: u32,
        child_error_mask: u32,
    },
    /// Case 69 (`446B90` -> `42E9B0`): destination offset (DWORD8/9).
    ConfigureTextureAuxiliaryPair {
        texture_table_base_offset: u32,
        max_parent_slot: u8,
        max_child_id: u16,
        error_flag_field_offset: u32,
        parent_error_mask: u32,
        child_error_mask: u32,
    },
    /// Case 71 (`446BE0` -> `42EA10`): draw order (DWORD10).
    ConfigureTextureAuxiliaryWord {
        texture_table_base_offset: u32,
        max_parent_slot: u8,
        max_child_id: u16,
        error_flag_field_offset: u32,
        parent_error_mask: u32,
        child_error_mask: u32,
    },
    /// Case 72 (`446B10` -> `42E8E0`): signed alpha with only an upper clamp.
    ConfigureTextureAlpha {
        texture_table_base_offset: u32,
        max_parent_slot: u8,
        max_child_id: u16,
        error_flag_field_offset: u32,
        parent_error_mask: u32,
        child_error_mask: u32,
    },
    /// The recovered case-321 handler (`sub_47F8D0`) clears the resource
    /// slot selected by the stack-top index (0..=7); out-of-range indices
    /// raise the slot error mask. Slot contents are unrecovered, so only
    /// membership is retained.
    ClearResourceSlot {
        max_slot: u8,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
    },
    /// The recovered case-538 handler (`sub_485F50`): delivers one click to
    /// the interactive chain selected by the stack-top bank index (0..=5)
    /// — the native routine hit-tests the chain's records against the
    /// pointer (or its forced fallback) and commits the first record whose
    /// rectangle contains it, reporting 1 when a selection was committed
    /// and 0 when nothing was hit. The headless host has no pointer; the
    /// chain's hover walk stands in for it. Returns 0x400c.
    ClickCommitFilterChain {
        max_bank: u32,
        result_field_offset: u32,
    },
    /// The recovered case-257 handler (`sub_488CF0`): validates the
    /// stack-top playback index (0..=11) and destroys the playback record
    /// at that index (freeing the native record and clearing the table
    /// entry). The headless playback record carries no native resources,
    /// so only the table entry is removed. Returns 0x4004.
    DestroyEffectPlayback {
        max_index: u32,
    },
    /// The recovered case-539 handler (`sub_486130`): validates the bank
    /// (0..=5) and channel (0..=255) stack words, frees the bank's previous
    /// hotspot record, and creates a fresh one bound to the channel with
    /// the stack position pair. The headless graph retains only the
    /// registration (the record id) — no pointer hover models the position.
    /// Returns 0x4010.
    CreateChainHotspotRecord {
        max_bank: u32,
        max_channel: u32,
        error_flag_field_offset: u32,
        error_mask: u32,
    },
    /// The recovered case-42 handler (`sub_481CD0`): validates the stack
    /// slot index against 0x100, binds the child element when the stack
    /// child id is non-negative (`sub_444EF0`), and stores the reference
    /// dimensions on the selected graphics object (`sub_445E80`).
    /// Its width controls the uniform stage-coordinate ratio in `445010`.
    /// Returns 0x4010.
    SetResourceSlotRegion {
        max_slot: u32,
    },
    /// Copies one recovered settings-object field into one interpreter
    /// word without touching the settings object, mirroring `sub_45A0E0`.
    CopySettingsFieldToInterpreterWord {
        settings_field_offset: u16,
        target_field_offset: u32,
    },
    /// Reads the settings-object field selected by the bounded stack index
    /// (`sub_45C300`) and stores its truthiness into one interpreter word.
    StoreIndexedSettingBoolean {
        target_field_offset: u32,
        max_index: u8,
    },
    /// Applies the recovered case-857 settings derivation (`sub_45C390`):
    /// zeroes the result field when both selector fields agree on zero, or
    /// when both are non-zero, then stores the result field and the second
    /// selector's truthiness into two interpreter words.
    StoreDerivedSettingsPair {
        first_selector_field_offset: u16,
        second_selector_field_offset: u16,
        result_field_offset: u16,
        result_target_field_offset: u32,
        flag_target_field_offset: u32,
    },
    /// Applies the recovered settings predicate (`sub_45C370`): the stack
    /// word must equal one and the named signed settings field must be
    /// non-negative; the boolean result goes to one interpreter word.
    StoreSettingsPredicate {
        settings_field_offset: u16,
        target_field_offset: u32,
    },
    /// Applies the recovered case-869 snapshot derivation (`sub_45C3E0`):
    /// when the gate field is non-zero the four gated source fields are
    /// copied into the four interpreter targets; one additional settings
    /// field is always copied, and the gate's truthiness lands in the result
    /// word.
    DeriveSettingsSnapshot {
        gate_field_offset: u16,
        copy_source_field_offsets: [u16; 4],
        copy_target_field_offsets: [u32; 4],
        always_source_field_offset: u16,
        always_target_field_offset: u32,
        gate_target_field_offset: u32,
    },
    /// Writes five stack words into one bounded 20-byte settings record
    /// (`sub_45B260`); out-of-range indices leave the table untouched.
    StoreSettingsRecord {
        record_base_field_offset: u16,
        record_stride_bytes: u8,
        max_index: u8,
    },
    /// Applies the recovered settings-object reset and stores two boolean
    /// settings at version-pinned field offsets.
    StoreSystemSettingBooleanPair {
        first_field_offset: u16,
        second_field_offset: u16,
    },
    /// Stores one stack word into a bounded settings field without the
    /// reset path; values outside `[minimum, maximum]` leave the field
    /// untouched, matching the original setter.
    StoreSystemSettingBoundedWord {
        setting_field_offset: u16,
        minimum: u32,
        maximum: u32,
    },
    /// Stores one non-negative signed stack word into a settings field
    /// without the reset path; negative values leave the field untouched.
    StoreSystemSettingNonNegativeWord {
        setting_field_offset: u16,
    },
    /// Sets the native save permission and its save-menu state (case 692).
    SetNativeSaveEnabled,
    /// Appends the resolved private string to the version-pinned string list
    /// proven by case 152 and writes the new list length to the named
    /// interpreter field. List node payloads stay in the script pool.
    /// Hands the recovered presentation-mode toggle to the host, naming the
    /// two interpreter fields the original layout helper consumes.
    TogglePresentationMode {
        first_field_offset: u16,
        second_field_offset: u16,
    },
    AppendPrivateStringToList {
        list_id: u16,
        length_field_offset: u32,
    },
    /// `sub_47CEE0` (case 307): copies the indexed entry of the
    /// this+0xC18 string list into the resolved destination slot; a short
    /// list copies the empty fallback constant.
    /// `sub_482940`/`sub_482A40`/`sub_482B70` (cases 490/491/492):
    /// install a payload on the this+0x784 channel-child presenter.
    /// `sub_4837F0` (72), `sub_4823E0` (500), `sub_4824D0` (501) and
    /// `sub_482600` (502): configure the this+0x784 channel-child
    /// presenter (quad creation, field writes and payload-table updates).
    ConfigureChannelChildPresenter {
        selector: u16,
        channel_table_field_offset: u32,
    },
    InstallChannelChildPayload {
        selector: u16,
        channel_table_field_offset: u32,
    },
    CopyPrivateListStringToSlot {
        list_id: u16,
        node_string_offset: u32,
    },
    /// `sub_47CEC0` (case 302): stores the this+0xC18 manager's count
    /// field into the pinned result word.
    StoreListCountToResult {
        list_id: u16,
    },
    /// `sub_47CE50` (case 309): stores the walked list length below the
    /// stack-top index (-1 for an empty list) into the pinned result word.
    StoreListWalkLengthToResult {
        list_id: u16,
    },
    /// Selects the system cursor through the cursor-manager component
    /// referenced by the named interpreter field. The current-index field of
    /// the manager is retained so unchanged selections stay effect-free,
    /// matching the original guard.
    SelectSystemCursor {
        manager_field_offset: u16,
        current_index_field_offset: u16,
    },
    /// Asks the host to load the system state archive assembled from the
    /// named path buffer. The nucleus cannot continue past the request
    /// until the host resolves it, matching the original dependency on the
    /// file contents.
    RequestSystemStateLoad {
        path_buffer_offset: u16,
    },
}

/// Independent native source tables. Unknown purposes retain their interpreter
/// field identity instead of merging tables that share an allocation shape.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub enum CmvsResourceSearchBank {
    /// Interpreter field 0xD08, configured by case 16.
    Script,
    /// Interpreter field 0xCFC, configured by case 17.
    Texture,
    /// Interpreter field 0xD10, configured by case 19.
    CrossfadeAudio,
    Native0d0c,
    Native0d14,
    Native0d00,
    Native0d04,
    Native0d18,
}

impl CmvsResourceSearchBank {
    pub fn slot_count(self) -> u8 {
        match self {
            Self::Texture | Self::Native0d00 => 10,
            Self::Native0d04 => 7,
            _ => 5,
        }
    }
}

/// The statically proven resource-channel banks. The bank fingerprint is the
/// original setup-handler address; the discriminant order is part of the
/// serialized contract.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub enum CmvsResourceChannelBank {
    /// Handler `sub_48AE50`; five channel records of 2052 bytes.
    FiveSlotSetup,
    /// Handler `sub_4405F0`; ten channel records of 2052 bytes.
    TenSlotSetup,
    /// Handler `sub_4331B0`; seven channel records.
    SevenSlotSetup,
    /// Handler `sub_441A70`; five channel records.
    FiveSlotAltSetup,
}

impl CmvsResourceChannelBank {
    /// The recovered slot count of this bank, taken from the original
    /// handler's bounds check.
    pub fn slot_count(self) -> u8 {
        match self {
            CmvsResourceChannelBank::FiveSlotSetup => 5,
            CmvsResourceChannelBank::TenSlotSetup => 10,
            CmvsResourceChannelBank::SevenSlotSetup => 7,
            CmvsResourceChannelBank::FiveSlotAltSetup => 5,
        }
    }
}

/// The proven representation of one four-byte word consumed by a CMVS 3.90
/// command handler.  These names deliberately describe the handler boundary,
/// rather than guessing game-level parameter names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsPs2aCommandStackWordKind {
    /// A value resolved by the original `sub_470D50` tagged-string resolver.
    /// Only tag-zero values can currently be materialized from the private
    /// PS2A string pool; other tag domains remain a runtime blocker.
    TaggedStringReference,
    /// A handler forwards this word without enough static evidence to assign a
    /// higher-level name or range.
    OpaqueU32,
    /// A handler uses this word as a zero/non-zero predicate.
    BooleanU32,
}

/// One word in the exact stack suffix a command handler consumes.  Offsets are
/// measured from the current stack top before the handler pop, so the topmost
/// word always has an offset of four bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsPs2aCommandStackWord {
    pub offset_from_top_bytes: u8,
    pub kind: CmvsPs2aCommandStackWordKind,
}

/// A fixed command stack contract. `stack_pop_bytes` is the low-byte pop
/// count returned by the original handler, not an inferred operand count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsPs2aCommandContract {
    pub command_id: u16,
    pub stack_pop_bytes: u8,
    pub effect_kind: CmvsPs2aCommandEffectKind,
    /// Static handler-derived stack word order. An empty slice means the
    /// handler does not pop stack data.
    pub stack_words: Vec<CmvsPs2aCommandStackWord>,
}

/// Looks up only CMVS 3.90 contracts recovered from the original command
/// dispatch and handler bodies.
/// Dispatcher cases whose handler is the shared `mov eax, 4000h` arm at
/// `0x472F45`: the command reads nothing, pops nothing and only advances the
/// program counter. Recovered from the `sub_472F10` jump table.
const NO_OP_COMMANDS: [u16; 246] = [
    36, 54, 58, 77, 82, 83, 87, 117, 118, 119, 121, 122, 123, 124, 126, 127, 132, 133, 134, 141,
    142, 149, 150, 151, 154, 155, 157, 158, 159, 187, 190, 191, 195, 199, 206, 207, 221, 222, 223,
    225, 226, 227, 229, 230, 234, 235, 237, 238, 254, 283, 292, 293, 295, 301, 312, 313, 317, 318,
    319, 362, 370, 371, 372, 373, 381, 392, 394, 396, 398, 399, 408, 409, 411, 419, 421, 423, 446,
    447, 452, 457, 462, 467, 472, 473, 474, 475, 476, 477, 510, 511, 512, 513, 514, 515, 516, 517,
    518, 519, 520, 521, 522, 523, 525, 526, 527, 540, 541, 542, 543, 581, 582, 588, 589, 590, 593,
    594, 595, 596, 597, 598, 599, 600, 601, 602, 603, 604, 605, 606, 607, 613, 614, 615, 623, 630,
    631, 632, 633, 634, 635, 636, 637, 638, 639, 655, 664, 665, 666, 667, 670, 671, 682, 683, 684,
    685, 703, 706, 707, 716, 758, 759, 762, 763, 766, 770, 771, 775, 792, 793, 799, 803, 804, 805,
    806, 807, 810, 811, 812, 813, 814, 815, 819, 820, 821, 822, 823, 827, 828, 829, 830, 831, 833,
    834, 835, 844, 862, 863, 867, 876, 877, 878, 879, 917, 921, 929, 930, 931, 937, 938, 939, 941,
    943, 944, 945, 947, 948, 950, 956, 957, 958, 959, 963, 964, 968, 969, 970, 971, 972, 973, 974,
    975, 976, 977, 978, 979, 980, 981, 982, 983, 984, 985, 986, 987, 988, 989, 990, 991,
];

pub fn cmvs390_command_contract(command_id: u16) -> Option<CmvsPs2aCommandContract> {
    if NO_OP_COMMANDS.contains(&command_id) {
        return Some(CmvsPs2aCommandContract {
            command_id,
            stack_pop_bytes: 0,
            effect_kind: CmvsPs2aCommandEffectKind::NoOpCommand,
            stack_words: Vec::new(),
        });
    }
    let (stack_pop_bytes, effect_kind, stack_words) = match command_id {
        608..=612 | 624..=629 => opcodes_530_741::lookup(command_id),
        256 | 257 | 258 | 259 | 260 | 261 | 262 | 263 | 264 | 267 | 268 | 275 | 285 | 287 => {
            native_text::lookup(command_id)
        }
        0 | 1 | 3 | 8 | 11 | 12 | 16 | 18 | 19 | 20 | 17 | 23 | 21 | 22 | 24 | 26 | 29 | 30
        | 31 | 32 | 33 | 34 | 35 | 39 | 40 | 38 | 41 | 42 | 44 | 46 | 47 | 48 | 51 | 52 | 59
        | 56 | 63 | 60 | 67 | 64 | 65 | 66 | 68 | 69 | 70 | 71 | 72 | 73 | 74 | 75 | 76 | 78
        | 193 => opcodes_0_71::lookup(command_id),
        80 | 84 | 85 | 86 | 88 | 89 | 90 | 91 | 92 | 93 | 94 | 95 | 96 | 97 | 98 | 99 | 100
        | 101 | 102 | 103 | 104 | 105 | 106 | 107 | 108 | 109 | 110 | 111 | 125 | 112 | 113
        | 114 | 115 | 116 | 128 | 129 | 135 | 136 | 137 | 183 => opcodes_80_137::lookup(command_id),
        138 | 139 | 140 | 143 | 144 | 145 | 146 | 147 | 148 | 152 | 153 | 156 | 160 | 161 | 162
        | 164 | 166 | 168 | 170 | 176 | 177 | 179 | 182 | 200 | 202 | 203 | 181 | 212 | 236
        | 243 | 248 | 252 | 269 | 270 | 271 | 272 | 276 | 277 | 278 | 279 | 282 | 286 | 294
        | 296 | 297 | 302 | 303 | 304 | 305 | 306 | 307 | 308 | 309 | 310 | 320 | 321 | 322
        | 332 | 338 | 339 | 341 | 196 | 197 | 198 | 186 => opcodes_138_322::lookup(command_id),
        323 | 324 | 325 | 326 | 327 | 328 | 329 | 330 | 331 | 335 | 333 | 334 | 336 | 337 | 343
        | 344 | 345 | 346 | 347 | 349 | 350 | 351 | 352 | 353 | 354 | 355 | 356 | 363 | 364
        | 365 | 369 => opcodes_323_354::lookup(command_id),
        368 | 376 | 377 | 378 | 379 | 380 | 382 | 383 | 384 | 385 | 386 | 387 | 388 | 389 | 390
        | 391 | 395 | 397 | 400 | 401 => opcodes_368_401::lookup(command_id),
        402 | 403 | 404 | 405 | 406 | 410 | 416 | 417 | 418 | 420 | 422 | 424 | 425 | 426 | 427
        | 428 | 429 | 430 | 431 | 432 | 433 | 463 | 464 | 465 | 466 | 468 | 469 | 470 | 471
        | 490 | 491 | 492 | 493 | 494 | 480 | 481 | 482 | 500 | 501 | 502 | 505 | 506 | 507
        | 508 | 509 | 524 | 528 | 529 | 860 => opcodes_402_529::lookup(command_id),
        530 | 531 | 532 | 533 | 534 | 535 | 538 | 539 | 544 | 545 | 548 | 549 | 550 | 551 | 552
        | 559 | 560 | 561 | 562 | 563 | 553 | 554 | 555 | 556 | 557 | 558 | 568 | 569 | 570
        | 571 | 573 | 574 | 575 | 592 | 640 | 641 | 644 | 647 | 649 | 650 | 672 | 674 | 680
        | 686 | 645 | 646 | 648 | 688 | 689 | 690 | 691 | 692 | 693 | 694 | 695 | 696 | 697
        | 698 | 699 | 700 | 701 | 702 | 704 | 705 | 708 | 709 | 710 | 715 | 717 | 718 | 719
        | 720 | 721 | 723 | 724 | 725 | 726 | 727 | 728 | 729 | 730 | 731 | 732 | 733 | 735
        | 713 | 714 | 736 | 737 | 739 | 740 | 741 | 743 | 744 | 745 | 760 | 761 => {
            opcodes_530_741::lookup(command_id)
        }
        747 | 748 | 734 | 749 | 772 | 773 | 774 | 750 | 751 | 762 | 764 | 765 | 776 | 847 | 777
        | 778 | 802 | 832 | 845 | 846 | 848 | 849 | 850 | 851 | 853 | 854 | 855 | 856 | 857
        | 858 | 859 | 864 | 866 | 779 | 780 | 781 | 782 | 783 | 794 | 795 | 796 | 797 | 798
        | 868 | 869 | 870 | 940 | 966 => opcodes_747_940::lookup(command_id),
        _ => native_table::lookup(command_id),
    }?;
    Some(CmvsPs2aCommandContract {
        command_id,
        stack_pop_bytes,
        effect_kind,
        stack_words: stack_words.to_vec(),
    })
}
