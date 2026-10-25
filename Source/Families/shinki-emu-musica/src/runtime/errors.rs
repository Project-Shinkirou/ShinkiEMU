use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MusicaRuntimeError {
    #[error("SHINKI_EMU_MUSICA_RUNTIME_STATE: runtime state is invalid")]
    State,
    #[error("SHINKI_EMU_MUSICA_RUNTIME_PC: program counter is outside the script")]
    ProgramCounter,
    #[error("SHINKI_EMU_MUSICA_RUNTIME_LABEL: branch label is missing or duplicated")]
    Label,
    #[error("SHINKI_EMU_MUSICA_RUNTIME_OPERAND: command operands do not match the verified schema")]
    Operand,
    #[error(
        "SHINKI_EMU_MUSICA_RUNTIME_OPCODE: command `{opcode}` at ordinal {ordinal} is not verified"
    )]
    UnsupportedOpcode { opcode: String, ordinal: u32 },
    #[error(
        "SHINKI_EMU_MUSICA_RUNTIME_NON_YIELDING_CYCLE: command execution repeated a control state without yielding"
    )]
    NonYieldingCycle,
    #[error("SHINKI_EMU_MUSICA_RUNTIME_WAIT: runtime is awaiting an unresolved token")]
    Waiting,
    #[error("SHINKI_EMU_MUSICA_RUNTIME_OVERFLOW: deterministic counter overflowed")]
    Overflow,
    #[error("SHINKI_EMU_MUSICA_NATIVE_SAVE_FORMAT: native save data is malformed")]
    NativeSaveFormat,
    #[error("SHINKI_EMU_MUSICA_RUNTIME_CHAIN: chain target is outside the script mount")]
    ChainTarget,
    #[error("SHINKI_EMU_MUSICA_RUNTIME_AUDIO_RESOURCE: audio resource specification is invalid")]
    AudioResource,
    #[error("SHINKI_EMU_MUSICA_RUNTIME_EFFECT: effect operands or timeline are invalid")]
    Effect,
    #[error("SHINKI_EMU_MUSICA_RUNTIME_PANEL: panel operands or mode are invalid")]
    Panel,
    #[error("SHINKI_EMU_MUSICA_RUNTIME_PRAGMA: unsupported or malformed pragma")]
    Pragma,
    #[error("SHINKI_EMU_MUSICA_RUNTIME_SCREEN_SHAKE: invalid screen shake command or state")]
    ScreenShake,
    #[error("SHINKI_EMU_MUSICA_RUNTIME_AXIS_SCROLL: invalid scroll command or state")]
    AxisScroll,
    #[error("SHINKI_EMU_MUSICA_RUNTIME_LINEAR_SCROLL: invalid linear scroll command or state")]
    LinearScroll,
    #[error("SHINKI_EMU_MUSICA_RUNTIME_SCROLL_XF: invalid scrollxf command or state")]
    ScrollXf,
    #[error("SHINKI_EMU_MUSICA_RUNTIME_WSCROLL2: invalid WScroll2 command or state")]
    WScroll2,
    #[error("SHINKI_EMU_MUSICA_RUNTIME_FIREFLY: invalid Firefly command or state")]
    Firefly,
    #[error("SHINKI_EMU_MUSICA_RUNTIME_CHARACTER: invalid character command or state")]
    Character,
    #[error("SHINKI_EMU_MUSICA_RUNTIME_BACKLOG: invalid or oversized backlog")]
    Backlog,
    #[error(transparent)]
    MessageMarkup(#[from] crate::MusicaMessageMarkupError),
    #[error(
        "SHINKI_EMU_MUSICA_RUNTIME_SECONDARY_EFFECT: invalid secondary effect command or state"
    )]
    SecondaryEffect,
}

impl MusicaRuntimeError {
    /// Stable diagnostic identity, independent of display text and source operands.
    pub fn diagnostic_code(&self) -> &'static str {
        match self {
            Self::State => "SHINKI_EMU_MUSICA_RUNTIME_STATE",
            Self::ProgramCounter => "SHINKI_EMU_MUSICA_RUNTIME_PC",
            Self::Label => "SHINKI_EMU_MUSICA_RUNTIME_LABEL",
            Self::Operand => "SHINKI_EMU_MUSICA_RUNTIME_OPERAND",
            Self::UnsupportedOpcode { .. } => "SHINKI_EMU_MUSICA_RUNTIME_OPCODE",
            Self::NonYieldingCycle => "SHINKI_EMU_MUSICA_RUNTIME_NON_YIELDING_CYCLE",
            Self::Waiting => "SHINKI_EMU_MUSICA_RUNTIME_WAIT",
            Self::Overflow => "SHINKI_EMU_MUSICA_RUNTIME_OVERFLOW",
            Self::NativeSaveFormat => "SHINKI_EMU_MUSICA_NATIVE_SAVE_FORMAT",
            Self::ChainTarget => "SHINKI_EMU_MUSICA_RUNTIME_CHAIN",
            Self::AudioResource => "SHINKI_EMU_MUSICA_RUNTIME_AUDIO_RESOURCE",
            Self::Effect => "SHINKI_EMU_MUSICA_RUNTIME_EFFECT",
            Self::Backlog => "SHINKI_EMU_MUSICA_RUNTIME_BACKLOG",
            Self::MessageMarkup(_) => "SHINKI_EMU_MUSICA_MESSAGE_MARKUP",
            Self::Character => "SHINKI_EMU_MUSICA_RUNTIME_CHARACTER",
            Self::Firefly => "SHINKI_EMU_MUSICA_RUNTIME_FIREFLY",
            Self::SecondaryEffect => "SHINKI_EMU_MUSICA_RUNTIME_SECONDARY_EFFECT",
            Self::WScroll2 => "SHINKI_EMU_MUSICA_RUNTIME_WSCROLL2",
            Self::ScrollXf => "SHINKI_EMU_MUSICA_RUNTIME_SCROLL_XF",
            Self::LinearScroll => "SHINKI_EMU_MUSICA_RUNTIME_LINEAR_SCROLL",
            Self::AxisScroll => "SHINKI_EMU_MUSICA_RUNTIME_AXIS_SCROLL",
            Self::ScreenShake => "SHINKI_EMU_MUSICA_RUNTIME_SCREEN_SHAKE",
            Self::Pragma => "SHINKI_EMU_MUSICA_RUNTIME_PRAGMA",
            Self::Panel => "SHINKI_EMU_MUSICA_RUNTIME_PANEL",
        }
    }
}
