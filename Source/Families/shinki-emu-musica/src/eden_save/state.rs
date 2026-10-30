use super::{EdenEdition, EdenHistoryMessage, EdenSaveEncoding};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Legacy v28 enum payload retained only for one-time `.asav` decoding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub enum EdenExportState {
    #[default]
    Unavailable,
    Ready {
        edition: EdenEdition,
        encoding: EdenSaveEncoding,
        history: Vec<EdenHistoryMessage>,
    },
    Rejected(EdenExportRejection),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum EdenExportRejection {
    Variables,
    Presentation,
    Audio,
    MessageBoundary,
    HistoryBound,
}
