pub(crate) mod camera_transition;
mod channel_input;
pub(crate) mod cmv;
mod dispatch;
pub(crate) mod image_effects;
pub(crate) mod independent_view;
pub(crate) mod message_history;
mod native_save;
pub use independent_view::{CmvsIndependentView, CmvsIndependentViewMotion};
pub(crate) mod native_mesh;
pub use native_mesh::{
    CmvsNativeMesh, CmvsNativeMeshVertex, CmvsNativeSnow, CmvsNativeSnowParticle,
};
pub(crate) mod native_hit_polygon;
pub use native_hit_polygon::{
    CmvsNativeCollisionGeometry, CmvsNativeCollisionParticle, CmvsNativeCollisionParticles,
};
pub(crate) mod native_scene_worker;
pub use native_scene_worker::CmvsNativeSceneWorker;
pub(crate) mod native_particle_kind1;
pub use native_particle_kind1::{CmvsNativeParticleKind1, CmvsNativeParticleKind1Particle};
pub(crate) mod native_particle_kind3;
pub use native_particle_kind3::{CmvsNativeParticleKind3, CmvsNativeParticleKind3Particle};
pub(crate) mod native_particle_kind4;
pub use native_particle_kind4::{CmvsNativeParticleKind4, CmvsNativeParticleKind4Particle};
pub(crate) mod native_particle_kind5;
pub use native_particle_kind5::{CmvsNativeParticleKind5, CmvsNativeParticleKind5Particle};
pub(crate) mod native_particle_kind7;
pub use native_particle_kind7::{CmvsNativeParticleKind7, CmvsNativeParticleKind7Particle};
pub(crate) mod native_particle_kind9;
pub use native_particle_kind9::{CmvsNativeParticleKind9, CmvsNativeParticleKind9Particle};
pub(crate) mod native_image_shards;
pub use native_image_shards::{CmvsNativeImageShardGrid, CmvsNativeImageShardParticle};
pub(crate) mod native_scene_image;
pub use native_scene_image::CmvsNativeImageShards;
pub(crate) mod native_scene_kind50;
pub use native_scene_kind50::CmvsNativeSceneKind50;
pub(crate) mod native_scene_frame;
pub(crate) mod native_scene_kind51;
pub use native_scene_kind51::CmvsNativeSceneKind51;
pub(crate) mod native_scene_kind52;
pub use native_scene_kind52::CmvsNativeSceneKind52;
pub(crate) mod native_scene_kind53;
pub use native_scene_kind53::CmvsNativeSceneKind53;
pub(crate) mod native_scene_kind54;
pub use native_scene_kind54::CmvsNativeSceneKind54;
pub(crate) mod native_scene_kind55;
pub use native_scene_kind55::CmvsNativeSceneKind55;
pub(crate) mod native_scene_kind56;
pub use native_scene_kind56::CmvsNativeSceneKind56;
pub(crate) mod native_scene_kind57;
pub use image_effects::{
    CmvsAlphaEffect, CmvsAlphaSegment, CmvsBasicEffects, CmvsFloatMotion, CmvsFloatSegment,
    CmvsSpriteEffect, CmvsSpriteFrame,
};
pub use native_scene_kind57::CmvsNativeSceneKind57;
pub(crate) mod native_glyph;
pub(crate) mod native_slot;
pub(crate) mod native_slot_projection;
pub(crate) mod native_text;
pub(crate) mod native_timeline;
pub use native_glyph::{
    CmvsNativeGlyphAtlas, CmvsNativeGlyphAuxiliary, CmvsNativeGlyphGraph, CmvsNativeGlyphMotion,
    CmvsNativeGlyphPainted, CmvsNativeGlyphRecord, CmvsNativeGlyphResident,
};
pub(crate) mod native_custom_glyph;
pub use native_custom_glyph::{CmvsNativeCustomGlyphBitmap, CmvsNativeCustomGlyphManager};
pub(crate) mod native_view;
pub(crate) mod screen_shake;
pub use native_save::{CmvsNativeSaveCatalog, CmvsNativeSaveSlot};
pub(crate) use native_text::layout::append as append_native_static_text;
pub use native_view::CmvsNativeViewState;
pub use screen_shake::CmvsScreenShakeState;
mod state;
pub(crate) mod texture_presentation;
pub(crate) mod texture_reload;
pub use dispatch::execute_cmvs390_frame;
pub use native_text::dialogue::CmvsNativeTextReveal;
pub use native_text::{
    CmvsNativeFontOwner, CmvsNativeFontRasterState, CmvsNativeTextAction, CmvsNativeTextGlyph,
    CmvsNativeTextSurface,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use shinki_emu_sdk::CoreError;
pub use state::CmvsPs2aVmState;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const NATIVE_FONT_SHIFT_JIS: u32 = 0x4e_a3a4;

use crate::{
    CmvsPs2aCommandEffectKind, CmvsPs2aCommandStackWordKind, CmvsPs2aControlInstruction,
    CmvsPs2aInstructionFrame, CmvsPs2aPrivateStringReference, CmvsResourceChannelBank,
    CmvsResourceSearchBank, cmvs390_command_contract,
};

/// The decoded-script byte budget shared with the PS2A parser; segment
/// sizes come from decoded scripts, so the same bound applies to the
/// snapshot they round-trip through.
const MAX_SCRIPT_BYTES: usize = 64 * 1024 * 1024;

const MAX_STACK_BYTES: usize = 64 * 1024;
const MAX_PROCESS_FLAG_BITS: u32 = 0x0004_0000;
const MAX_PROCESS_INDEXED_WORDS: u32 = 0x0000_1000;
const MAX_PROCESS_FLOAT_WORDS: u32 = 0x0000_0800;
const MAX_PROCESS_STRING_SLOTS: u32 = 0x0000_0080;
/// Bounds proven by the CMVS 3.90 slot-table handlers; indices at or above
/// this limit only raise the recovered interpreter error flag.
const SLOT_OBJECT_COUNT: u32 = 256;
/// Byte offset of the interpreter error-flag word OR-ed by the recovered
/// slot-table bounds checks (`this+2633` on the original object layout).
const INTERPRETER_ERROR_FLAG_OFFSET: u16 = 10532;
const INTERPRETER_ERROR_FLAG_MASK: u32 = 0x10;
const MAX_FILTER_CHAIN_RECORDS_PER_BANK: usize = 4096;

/// Validates snapshot-carried interpreter memory before it can resume live
/// execution. The wire payload is untrusted even when its outer envelope and
/// hash are valid.
pub fn validate_cmvs390_vm_state(state: &CmvsPs2aVmState) -> Result<(), CoreError> {
    state.message_history.validate()?;
    camera_transition::validate(state)?;
    independent_view::validate_pair(&state.independent_views)?;
    native_scene_worker::validate(state)?;
    native_hit_polygon::validate(state)?;
    image_effects::validate(state)?;
    native_text::validate(state)?;
    native_save::validate(state)?;
    texture_reload::validate(state)?;
    cmv::validate(state)?;
    if state
        .texture_presentation
        .as_ref()
        .is_some_and(|p| p.elapsed_ms > p.duration_ms)
    {
        return Err(invalid(
            "SHINKI_EMU_CMVS_PRESENTATION_CLOCK",
            "presentation elapsed time exceeds its duration",
        ));
    }
    crate::system_save::validate_lists(&state.native_system_lists)?;
    for (bank, paths) in &state.resource_search_paths {
        for (slot, path) in paths {
            if *slot >= bank.slot_count() {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_RESOURCE_SLOT",
                    "resource search slot exceeds native table",
                ));
            }
            crate::archive::native::validate_search_path(path)?;
        }
    }
    for (index, bytes) in &state.process_string_slots {
        if *index >= MAX_PROCESS_STRING_SLOTS {
            return Err(invalid(
                "SHINKI_EMU_CMVS_VM_STRING_BOUND",
                "string slot index exceeds native table",
            ));
        }
        validate_native_string_slot(bytes)?;
    }
    if state.message_buffer.len() > MAX_SCRIPT_BYTES {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_STRING_BOUND",
            "string evaluator buffer exceeds byte budget",
        ));
    }
    if state.execution_failed {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_FAILED",
            "failed VM state cannot be saved or restored",
        ));
    }
    if state.stack_bytes.len() > MAX_STACK_BYTES
        || state.stack_initialized.len() != state.stack_bytes.len()
    {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_STACK_LIMIT",
            "CMVS snapshot variable-buffer shape is invalid",
        ));
    }
    let cursor = usize::try_from(state.stack_cursor_bytes).map_err(|_| {
        invalid(
            "SHINKI_EMU_CMVS_VM_STACK_LIMIT",
            "CMVS snapshot stack cursor exceeds platform bounds",
        )
    })?;
    if cursor > state.stack_bytes.len() {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_STACK_LIMIT",
            "CMVS snapshot stack cursor exceeds its variable buffer",
        ));
    }
    if state.call_frame_bases.is_empty()
        || state.call_frame_bases[0] != 0
        || state.call_frame_bases.len() > MAX_STACK_BYTES / 4
        || state
            .call_frame_bases
            .windows(2)
            .any(|bases| bases[0] > bases[1])
        || state
            .call_frame_bases
            .last()
            .is_some_and(|base| *base > state.stack_cursor_bytes)
    {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_CALL_DEPTH",
            "CMVS snapshot call-frame bases are invalid",
        ));
    }
    for parent in state
        .texture_parents
        .keys()
        .chain(state.texture_children.keys())
    {
        if !state
            .slot_objects
            .contains_key(&slot_object_key(1924, u32::from(*parent))?)
        {
            return Err(invalid(
                "SHINKI_EMU_CMVS_VM_TEXTURE",
                "CMVS snapshot texture state has no live parent object",
            ));
        }
    }
    if state.texture_parents.values().any(|parent| {
        parent.resource.is_some() != parent.resource_frame.is_some()
            || parent
                .resource_frame
                .is_some_and(|frame| !state.script_frames.contains_key(&frame))
    }) || state.texture_children.values().flatten().any(|(_, child)| {
        child.resource.is_some() != child.resource_frame.is_some()
            || child
                .resource_frame
                .is_some_and(|frame| !state.script_frames.contains_key(&frame))
    }) {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_TEXTURE_IDENTITY",
            "CMVS snapshot texture binding has no matching script-frame identity",
        ));
    }
    // The data-segment budget mirrors the script decode bound; recorded
    // words must stay inside the loaded segment declared for their frame.
    for (frame, size) in &state.script_data_segment_sizes {
        if u64::from(*size) > MAX_SCRIPT_BYTES as u64 {
            return Err(invalid(
                "SHINKI_EMU_CMVS_VM_DATA_SEGMENT",
                "CMVS snapshot data-segment size exceeds the script budget",
            ));
        }
        if *size > 0 && !state.script_frames.contains_key(frame) {
            return Err(invalid(
                "SHINKI_EMU_CMVS_VM_DATA_SEGMENT",
                "CMVS snapshot data-segment size has no matching script frame",
            ));
        }
    }
    for (frame, tail) in &state.script_data_segment_tail {
        if state
            .script_data_segment_sizes
            .get(frame)
            .is_none_or(|size| tail.len() != (*size as usize & 3))
            || tail.is_empty()
        {
            return Err(invalid(
                "SHINKI_EMU_CMVS_VM_DATA_SEGMENT",
                "data segment tail does not match its declared size",
            ));
        }
    }
    for (frame, size) in &state.script_data_segment_sizes {
        if *size & 3 != 0 && !state.script_data_segment_tail.contains_key(frame) {
            return Err(invalid(
                "SHINKI_EMU_CMVS_VM_DATA_SEGMENT",
                "data segment tail is missing",
            ));
        }
    }
    for (frame, words) in &state.script_data_segment_words {
        let Some(size) = state.script_data_segment_sizes.get(frame) else {
            return Err(invalid(
                "SHINKI_EMU_CMVS_VM_DATA_SEGMENT",
                "CMVS snapshot data-segment words have no declared segment size",
            ));
        };
        for offset in words.keys() {
            let end = offset.checked_add(4).ok_or_else(|| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_DATA_SEGMENT",
                    "CMVS snapshot data-segment word offset overflowed",
                )
            })?;
            if end > *size {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_VM_DATA_SEGMENT",
                    "CMVS snapshot data-segment word is outside the loaded segment",
                ));
            }
        }
    }
    Ok(())
}

/// One recovered resource channel slot record from the case-176 handler.
/// Native 48A110 copies the source name into its fixed descriptor; it does
/// not retain a reference into the mutable script string pool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsResourceChannelSlotRecord {
    pub start_playback: bool,
    pub volume: u32,
    pub loop_playback: bool,
    pub source: String,
}

/// Recovered state of one child texture below a case-32 container.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsTextureChildState {
    pub resource: Option<String>,
    pub resource_frame: Option<u16>,
    pub image_info: [u32; 3],
    pub surface_initialized: bool,
    /// Inverse of native visibility DWORD1035; fresh roots and children are visible.
    pub hidden: bool,
    /// Header bit 0 cleared by command 44; basic rebuilds preserve it.
    pub parent_context_detached: bool,
    /// Header 0x100 cleared by command 46; parent header controls child shaking.
    pub screen_shake_disabled: bool,
    /// Native basic-surface alpha; None retains the default 255.
    pub alpha: Option<i32>,
    /// Packed opaque solid color set by command 67; None retains the native default.
    pub solid_color: Option<u32>,
    /// Raw IEEE754 scale words from command 73; None is native (1, 1).
    pub scale_words: Option<[u32; 2]>,
    /// Native basic DWORD0; 0/1/5 are flat, 2/3 use the corresponding camera.
    pub basic_kind: u8,
    /// None retains native alpha blending (DWORD1 = 1).
    pub blend_mode: Option<u8>,
    /// Native floating position DWORD11..13, independent of integer DWORD8/9.
    pub float_position_words: [u32; 3],
    pub projection_depth_word: u32,
    pub projection_height_word: u32,
    /// Native CBasic DWORD20, signed hundredths of a degree.
    pub rotation_hundredths: i32,
    pub reference_size: Option<[u32; 2]>,
    /// Source rectangle (x, y, width, height).
    pub rect_words: Option<[u32; 4]>,
    /// Pivot (DWORD6/7), set by command 70; independent of source UV.
    pub position_words: Option<[u32; 2]>,
    /// Destination offset (DWORD8/9), set by command 69.
    pub auxiliary_pair_words: Option<[u32; 2]>,
    /// Draw order (DWORD10), set by command 71.
    pub auxiliary_word: Option<u32>,
    pub basic_effects: CmvsBasicEffects,
    /// Disabled CImage effect storage is stale but survives native capture.
    pub image_effect_words: [u32; 11],
    /// CImage DWORD1031, independent of its drawable basic.
    pub collision_geometry: Option<CmvsNativeCollisionGeometry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsTextureParentState {
    pub resource: Option<String>,
    pub resource_frame: Option<u16>,
    /// CTextureInfo width, height and source-alpha flag queried by command 51.
    /// An allocated provider whose load failed retains the zero-initialized info.
    pub image_info: [u32; 3],
    pub surface_initialized: bool,
    pub hidden: bool,
    pub screen_shake_disabled: bool,
    /// Native basic-surface alpha; None retains the default 255.
    pub alpha: Option<i32>,
    /// Packed opaque solid color set by command 67; None retains the native default.
    pub solid_color: Option<u32>,
    pub scale_words: Option<[u32; 2]>,
    pub basic_kind: u8,
    pub blend_mode: Option<u8>,
    pub float_position_words: [u32; 3],
    pub projection_depth_word: u32,
    pub projection_height_word: u32,
    /// Native CBasic DWORD20, signed hundredths of a degree.
    pub rotation_hundredths: i32,
    /// Native DWORD1038/1039; width determines the stage coordinate ratio.
    pub reference_size: Option<[u32; 2]>,
    /// Source rectangle (x, y, width, height).
    pub rect_words: Option<[u32; 4]>,
    /// Source offset (DWORD6/7), set by command 70.
    pub position_words: Option<[u32; 2]>,
    /// Destination offset (DWORD8/9), set by command 69.
    pub auxiliary_pair_words: Option<[u32; 2]>,
    /// Draw order (DWORD10), set by command 71.
    pub auxiliary_word: Option<u32>,
    /// Native `sub_43C2A0` image mode, packed BGR color, and row kernel.
    pub color_filter: Option<[u32; 3]>,
    pub basic_effects: CmvsBasicEffects,
    pub image_effect_words: [u32; 11],
    pub collision_geometry: Option<CmvsNativeCollisionGeometry>,
}

/// One keyed record allocated by the recovered filter-chain helper.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsFilterChainRecord {
    pub words: [u32; 4],
    pub parameter_blocks: BTreeMap<u8, CmvsFilterChainParameterBlock>,
}

/// One physical 16-byte parameter block in a filter-chain record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsFilterChainParameterBlock {
    pub word: u32,
    pub short_words: [u16; 6],
}

/// The serialized identity of one loaded script frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsScriptFrameIdentity {
    pub script_uri: String,
    pub script_hash: roukaku_core::Hash256,
}

/// The recovered maximum nested-script frame index accepted by `sub_478080`.
pub const CMVS_MAX_SCRIPT_CALL_INDEX: u32 = 3;

/// One core-private segment of a recovered interpreter string buffer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsStringSegment {
    /// An interpreter-owned prefix field copied before any script string.
    /// The field content is engine state and remains unrecovered.
    InterpreterPrefixField { field_offset: u16 },
    /// A resolved tag-zero private reference into the active script pool.
    PrivateString(CmvsPs2aPrivateStringReference),
    /// An immediately copied core-private value, independent of script pools.
    OwnedString(String),
}

/// A host storage request emitted by a recovered command. The nucleus keeps
/// dispatch blocked until the runtime provider resolves the request; the
/// request itself carries no payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsPs2aStorageRequest {
    /// The recovered case-296 path: load the system state archive assembled
    /// from the interpreter path buffer at the named byte offset.
    LoadSystemState { path_buffer_offset: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsNativeAudioControl {
    Start { looping: bool },
    Restart { looping: bool },
    Pause,
    Dispose,
    Volume(u32),
    Pan(i32),
    FadeOut { duration_ms: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsNativeAudioQuery {
    Position,
    DecodedBytes,
    Levels,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsNativeAudioAction {
    LoadVoice {
        channel: u8,
        name: String,
        volume: u32,
        looping: bool,
        rate_bits: u32,
    },
    Control {
        channel: u8,
        control: CmvsNativeAudioControl,
    },
    Query {
        channel: u8,
        query: CmvsNativeAudioQuery,
    },
    LoadFixedStream {
        name: String,
        volume: u32,
        fade_in_ms: u32,
        looping: bool,
        start: bool,
    },
    LoadStatic {
        slot: u8,
        name: String,
    },
    FreeStatic {
        slot: u8,
    },
    PlayStatic {
        slot: u8,
        volume: u32,
        looping: bool,
    },
    StopStatic {
        slot: u8,
    },
}

/// Core-private command boundary. Resolved names stay within the Family;
/// callers must not serialize actions into diagnostics or public reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsPs2aVmAction {
    NativeAudio(CmvsNativeAudioAction),
    NativeText(CmvsNativeTextAction),
    CreateNativeScene53 {
        slot: u8,
        retired_static_sounds: u32,
        stopped_streams: u16,
    },
    NativeSceneWorkerResourceChanged {
        slot: u8,
        retired_static_sounds: u32,
        stopped_streams: u16,
    },
    TransferTextureParentToSceneWorker {
        parent_slot: u8,
        slot: u8,
        retired_static_sounds: u32,
        stopped_streams: u16,
    },
    SetNativeFullscreen {
        enabled: bool,
    },
    /// Cases 729..733: the canonical gain word has already been stored.
    ApplyNativeAudioGainSetting {
        command_id: u16,
    },
    /// Cases 160/164: alternate two audio channels, optionally crossfading.
    PlayCrossfadeAudio {
        primary: CmvsPs2aPrivateStringReference,
        secondary: Option<CmvsPs2aPrivateStringReference>,
        fade_ms: u32,
        playback_flag: bool,
    },
    /// Case 162: fade out the active channel and clear its resource names.
    FadeOutAudio {
        fade_ms: u32,
    },
    /// Case 161: stop both channels and reset the channel selector.
    StopAudio,
    /// Case 176: load the named stream on native channel 9..14.
    ConfigureResourceAudio {
        slot: u8,
        fade_in_ms: u32,
    },
    /// Advance the native timeline after its command updates its targets.
    FlushNativeTimeline,
    BuildNativeSaveSnapshot {
        resume_pc: u32,
    },
    OpenCmvResource {
        controller_slot: u8,
        parent_slot: u8,
        resource: String,
        start_frame: u32,
    },
    PresentCmvFrame {
        controller_slot: u8,
        frame: u32,
    },
    ReleaseCmvResource {
        controller_slot: u8,
    },
    /// Case 73/193: the single object at one dword index is released.
    ReleaseSingleObject {
        dword_index: u32,
    },
    /// Case 349: the effect playback objects for one channel/effect pair
    /// were rebuilt; the media graph itself stays unrecovered.
    CreateEffectChannel {
        channel: u32,
        effect: u32,
    },
    /// Case 347: one effect channel's enable flag was forwarded to the
    /// effect engine.
    SetEffectChannelEnabled {
        channel: u32,
        enabled: bool,
    },
    /// Case 322: one opaque word was forwarded to an occupied effect
    /// channel. The native sink stores the value at dwords 18/19 of the
    /// channel's element container (`sub_467A50` → `sub_42EA20`); the game
    /// drives per-frame fade and animation levels through it.
    ForwardEffectChannelWord {
        channel: u32,
        value: u32,
    },
    /// Case 328: two opaque words were forwarded to an occupied channel.
    ForwardEffectChannelPair {
        channel: u32,
    },
    /// Case 325: four opaque words were forwarded to an occupied channel.
    ForwardEffectChannelQuad {
        channel: u32,
    },
    /// Case 350: the effect position was queried into two result words.
    QueryEffectPosition,
    /// Case 400: one effect quad configuration was applied.
    ConfigureEffectQuad {
        channel: u32,
    },
    /// Case 351: the pointer hit test ran and its canonical result was
    /// stored.
    QueryEffectPointerHit,
    /// Case 402: quad `quad` on the occupied channel was shown.
    SelectEffectQuad {
        channel: u32,
        quad: u32,
    },
    /// Case 403: quad `quad` on the occupied channel was hidden.
    DeselectEffectQuad {
        channel: u32,
        quad: u32,
    },
    /// Case 404: the effect state was queried into three result words.
    QueryEffectState,
    /// Case 405: the quad existence query stored its canonical result.
    QueryEffectQuadActive,
    /// Case 378: a child of an occupied channel received a boolean.
    SetEffectChildEnabled {
        channel: u32,
        enabled: bool,
    },
    SelectEffectChild {
        channel: u32,
    },
    LoadChannelImage {
        channel: u8,
        resource: String,
        load_error_mask: u32,
    },
    /// A recovered resource-channel setup request. This is not yet bound to a
    /// media provider; the host must reject an unbound channel contract.
    StartResourceChannel {
        bank: CmvsResourceChannelBank,
        channel: u8,
        resource: CmvsPs2aPrivateStringReference,
    },

    /// The recovered case-56 path binds a private PB resource reference to
    /// one child texture. The host must resolve it through the active VFS and
    /// explicitly bound CMVS image decoder before presenting it.
    LoadTextureResource {
        parent_slot: u8,
        child_id: u16,
        resource: String,
    },
    /// Case 960 appends a type-8 bitmap sheet to the native glyph manager.
    LoadCustomGlyphResource {
        resource: String,
        cell_size: u32,
    },
    /// The recovered case-48 path binds a private PB resource directly to a
    /// top-level texture container.
    LoadTextureParentResource {
        parent_slot: u8,
        resource: String,
    },
    /// Case 708 replaces the root before attempting the image load.
    ReloadTextureParentImage {
        parent_slot: u8,
        resource: String,
        bank: CmvsResourceSearchBank,
        result_field_offset: u32,
    },
    /// Case 709 drops both transition owners and leaves the new image.
    ReleaseTextureReload {
        parent_slot: u8,
    },
    /// Case 40 moves the existing image owner, including an active reload.
    MoveTextureParentSurface {
        source: u8,
        destination: u8,
    },
    /// The recovered case-966 path opens a scenario stream resource by name;
    /// the host resolves it against the mounted archive and the load
    /// completes synchronously before the next frame.
    OpenScenarioStream {
        resource: String,
    },
    /// The recovered case-196 path opens the named movie stream; the host
    /// resolves it against the mounted archives and reports the open result
    /// through the named interpreter word.
    OpenMovieStream {
        resource: String,
        result_field_offset: u32,
    },
    /// The recovered case-696 path captures the presented frame as the save
    /// snapshot; the host owns the framebuffer, so the capture happens here.
    CaptureSaveSnapshot,
    /// The recovered case-693 path loads the named save slot and switches
    /// the running program; the host owns the native save files.
    LoadNativeSaveSlot {
        slot: u32,
    },
    /// The recovered case-694 path writes the running program state into
    /// the named save slot.
    SaveNativeGameSlot {
        slot: u32,
    },
    /// The recovered case-699 path probes the save slot file for the CSV
    /// magic; the result lands in the named interpreter word.
    ProbeNativeSaveSlot {
        slot: u32,
        result_field_offset: u32,
    },
    /// The recovered case-700 path rewrites the save header title.
    SetNativeSaveTitle {
        slot: u32,
        title: String,
        result_field_offset: u32,
    },
    /// The recovered case-297 path serializes the system state through
    /// `sub_427E80` into `save/system.dat`.
    SaveSystemState,
    /// The recovered case-701 path reads the save slot's timestamp, title,
    /// metadata and thumbnail; failures publish zero.
    LoadNativeSaveInfo {
        slot: u32,
        parent: u32,
        child: i32,
        title_target: u32,
        secondary_target: u32,
        result_field_offset: u32,
    },
    /// The recovered case-702 path rewrites the save header metadata pair.
    SetNativeSaveMeta {
        slot: u32,
        metadata: [u32; 2],
        result_field_offset: u32,
    },
    /// Resolve the active owner's mask without replacing its source image.
    PrepareTexturePresentation {
        clear_background: bool,
    },
    /// A destroyed or rebuilt native parent releases its complete surface family.
    RemoveTextureParentSurface {
        parent_slot: u8,
    },
    ClearTextureParentSurfaces,
    RemoveTextureChildSurface {
        parent_slot: u8,
        child_id: u16,
    },
    /// Case 71 completes the recovered four-stage surface configuration.
    /// The host may present the selected parent or child only after this
    /// boundary; preceding setters remain state-only mutations.
    CommitTextureSurface {
        parent_slot: u8,
        child_id: Option<u16>,
    },
    /// The recovered case-22 path asks the host to create the directories
    /// along the assembled path buffer. The host owns the storage policy;
    /// the VM never touches the filesystem itself.
    EnsurePathDirectories {
        buffer_offset: u16,
    },
    /// An owned caption to display through the Family window command.
    SetWindowIcon {
        segments: Vec<CmvsStringSegment>,
    },
    SetWindowCaption {
        segments: Vec<CmvsStringSegment>,
    },
    /// The recovered case-354 path asks the host to resolve the reference,
    /// apply the bounded buffer write, or raise the error-flag mask when the
    /// resolved text exceeds the bound.
    StoreBoundedString {
        buffer_offset: u16,
        max_text_bytes: u16,
        error_flag_field_offset: u16,
        error_flag_mask: u32,
        reference: CmvsPs2aPrivateStringReference,
    },
    /// The recovered case-691 path asks the host to apply the window layout
    /// derived from the stored presentation size field.
    ApplyPresentationLayout {
        size_field_offset: u16,
    },
    /// The recovered case-692 path asks the host to toggle the presentation
    /// mode using the two named interpreter fields.
    TogglePresentationMode {
        first_field_offset: u16,
        second_field_offset: u16,
        enabled: bool,
    },
    /// Read the native save-slot catalog without creating or modifying saves.
    ScanNativeSaveSlots {
        count: u16,
    },
    /// The recovered case-153 path asks the host to select the system cursor
    /// with the given index.
    SelectSystemCursor {
        index: u32,
    },
    /// The recovered case-296 path asks the host to load the system state
    /// archive; dispatch waits for the provider resolution.
    StorageRequest(CmvsPs2aStorageRequest),
    /// The recovered case-548 path is waiting for a confirm or primary
    /// pointer edge before it commits the filter-graph control.
    FilterGraphInputWait,
    /// The recovered case-535 selection poll waits for a directional,
    /// confirm, cancel, or primary-pointer edge owned by the host.
    FilterChainSelectionWait {
        bank: u8,
    },
    /// The recovered case-129 path asks the host to load the named script
    /// into the given frame and continue dispatch there.
    CallScript {
        frame: u16,
        name: String,
    },
    /// The recovered case-128 path asks the host to load the named script as
    /// the new root frame-0 script and continue dispatch at its entry PC.
    ReloadRootScript {
        name: String,
    },
}

mod effect;
mod execute;
mod expression;
mod filterchain;
mod memory;
mod numeric;
mod runtime;
mod schedule;
mod stack;
mod system_state;
mod text;

pub use self::effect::CmvsEffectChannel;
pub(crate) use self::effect::{CmvsEffectElement, CmvsEffectPlaybackRecord, CmvsEffectTextSurface};
use self::execute::*;
use self::expression::*;
use self::filterchain::*;
use self::memory::*;
use self::numeric::*;
use self::runtime::*;
use self::schedule::*;
use self::stack::*;

pub(crate) use self::text::resolve_resource_name;
use self::text::*;

pub use self::schedule::rebuild_frame_entry_queue;
pub use self::system_state::apply_system_save;

fn invalid(code: &'static str, message: &'static str) -> CoreError {
    CoreError::invalid(code, message)
}

#[cfg(test)]
mod tests;
