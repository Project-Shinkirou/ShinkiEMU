use std::fmt;

pub const MAX_SYMBOL_BYTES: usize = 256;
pub const MAX_GAME_PATH_BYTES: usize = 4096;
pub const MAX_EVENTS_PER_ADVANCE: usize = 4096;
pub const MAX_TEXT_BYTES: usize = 32 * 1024;

/// Error owned by a native runtime. The code is stable; the message is
/// for a human and must not contain game text, secrets, or host paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionError {
    pub code: String,
    pub message: String,
}

impl SessionError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn invalid(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(code, message)
    }

    pub fn code(&self) -> &str {
        self.code.as_str()
    }
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SessionError {}

pub type SessionResult<T> = Result<T, SessionError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeCapability {
    CpuFrame,
    PcmAudio,
    NativeSave,
    TextReplacement,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeDescriptor {
    pub family_id: String,
    pub plugin_id: String,
    pub version: String,
    pub capabilities: Vec<RuntimeCapability>,
    pub supported_formats: Vec<String>,
    pub configuration: Vec<super::ConfigField>,
}

impl RuntimeDescriptor {
    pub fn validate(&self) -> SessionResult<()> {
        super::validate_config_schema(&self.configuration)?;
        validate_symbol("family_id", &self.family_id)?;
        validate_symbol("plugin_id", &self.plugin_id)?;
        validate_symbol("version", &self.version)?;
        if self.capabilities.is_empty() || !self.capabilities.contains(&RuntimeCapability::CpuFrame)
        {
            return Err(SessionError::invalid(
                "SHINKI_EMU_FAMILY_CAPABILITIES",
                "family must declare CpuFrame capability",
            ));
        }
        for (index, capability) in self.capabilities.iter().enumerate() {
            if self.capabilities[..index].contains(capability) {
                return Err(SessionError::invalid(
                    "SHINKI_EMU_FAMILY_DUPLICATE_CAPABILITY",
                    "family descriptor contains a duplicate capability",
                ));
            }
        }
        validate_unique_symbols("supported_formats", &self.supported_formats)?;
        if self.supported_formats.is_empty() {
            return Err(SessionError::invalid(
                "SHINKI_EMU_FAMILY_FORMATS",
                "family must declare at least one supported game format",
            ));
        }
        Ok(())
    }

    pub fn has_capability(&self, capability: RuntimeCapability) -> bool {
        self.capabilities.contains(&capability)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeRequest {
    /// The only game resource crossing the contract. The family owns all
    /// reads below this directory and must not receive a host VFS object.
    pub game_path: String,
}

impl ProbeRequest {
    pub fn validate(&self) -> SessionResult<()> {
        validate_game_path(&self.game_path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    pub family_id: String,
    /// Opaque provider-defined ID. It may contain Unicode and is not used as
    /// a log field; a no-match is represented by `Option<ProbeReport>`.
    pub game_id: String,
    pub format: String,
    pub confidence_permyriad: u16,
}

impl ProbeReport {
    pub fn validate(&self) -> SessionResult<()> {
        validate_symbol("family_id", &self.family_id)?;
        if self.game_id.is_empty() || self.game_id.len() > MAX_SYMBOL_BYTES {
            return Err(SessionError::invalid(
                "SHINKI_EMU_FAMILY_GAME_ID",
                "game ID must be non-empty and within the ID bound",
            ));
        }
        validate_symbol("format", &self.format)?;
        if self.confidence_permyriad > 10_000 {
            return Err(SessionError::invalid(
                "SHINKI_EMU_FAMILY_PROBE_CONFIDENCE",
                "probe confidence exceeds 10000 permyriad",
            ));
        }
        Ok(())
    }
}

pub(crate) fn validate_symbol(name: &str, value: &str) -> SessionResult<()> {
    if value.is_empty() || value.len() > MAX_SYMBOL_BYTES || !value.is_ascii() {
        return Err(SessionError::invalid(
            "SHINKI_EMU_FAMILY_SYMBOL",
            format!("{name} must be non-empty ASCII within the symbol bound"),
        ));
    }
    Ok(())
}

pub(crate) fn validate_unique_symbols(name: &str, values: &[String]) -> SessionResult<()> {
    for (index, value) in values.iter().enumerate() {
        validate_symbol(&format!("{name}[{index}]"), value)?;
        if values[..index].iter().any(|other| other == value) {
            return Err(SessionError::invalid(
                "SHINKI_EMU_FAMILY_DUPLICATE",
                format!("{name} contains a duplicate symbol"),
            ));
        }
    }
    Ok(())
}

pub fn validate_game_path(path: &str) -> SessionResult<()> {
    if path.is_empty() || path.len() > MAX_GAME_PATH_BYTES || path.contains('\0') {
        return Err(SessionError::invalid(
            "SHINKI_EMU_FAMILY_GAME_PATH",
            "game path is empty, too long, or contains NUL",
        ));
    }
    Ok(())
}
