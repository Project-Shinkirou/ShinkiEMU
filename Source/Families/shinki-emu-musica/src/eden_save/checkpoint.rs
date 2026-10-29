//! The native message checkpoint subset whose fields have explicit semantics.
//! Opaque fields never survive this conversion: unsupported state is rejected.
use super::*;
use std::collections::BTreeMap;

#[derive(Clone, PartialEq, Eq)]
pub struct EdenCheckpoint {
    pub script: String,
    pub next_line: u32,
    pub message_id: i64,
    pub background: String,
    pub background_position: [i32; 2],
    pub panel_mode: u32,
    pub panel_resource: String,
    pub bgm: String,
    pub bgm_volume: u16,
    pub transition_ticks: u32,
    pub history: Vec<EdenHistoryMessage>,
}

/// Historical presentation is retained as named native state, not an opaque blob.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct EdenHistoryMessage {
    pub script: String,
    pub next_line: u32,
    pub message_id: i64,
    /// `CS3` when the original writer stored a value different from `L0`.
    /// Only a trailing novel page with no text has this: eden0005 record 6098
    /// holds `L0=-1`, `CS3=4140`, `LC=2` and empty `L1_n..L3_n`, written after
    /// `.message 4140` / `.panel 0` / `.panel 2` in C006.sc lines 714-718.
    /// Native slots carry it; the runtime snapshot does not.
    #[serde(skip)]
    pub state_message_id: Option<i64>,
    /// Plain `L1`/`L2`/`L3`; empty for a novel page (`CP1=2`), whose lines are in `page`.
    pub voice: String,
    pub speaker: String,
    pub text: String,
    /// Novel-mode (`CP1=2`) page lines `L1_n..L4_n`, `LC` of them; empty otherwise.
    /// Native slots carry the page; the runtime snapshot does not.
    #[serde(skip)]
    pub page: Vec<EdenHistoryLine>,
    pub background: String,
    pub background_position: [i32; 2],
    pub panel_mode: u32,
    pub panel_resource: String,
    pub bgm: String,
    pub bgm_volume: u16,
    pub sound_effects: [String; 2],
    /// CT5: number of stand-name slots, written by sub_4C9C90 as
    /// `(end - begin) / 28` over a vector of 28-byte MSVC `std::string`s.
    /// CT6_%d/CT7_%d/CT7Y_%d follow only for non-empty names (size at +16).
    pub stand_count: u32,
    pub transition_ticks: u32,
    /// Native fire-effect resource (Ce1): "", "Firefly", "end".
    pub fire_effect: String,
    /// Native transition method (Ct1): 0 or 1.
    pub transition_method: u32,
}

impl std::fmt::Debug for EdenHistoryMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EdenHistoryMessage")
            .field("message_id", &self.message_id)
            .finish_non_exhaustive()
    }
}

/// One line of a novel-mode page. The original CLog writer `sub_4F1920`
/// (CLog vtable +72 at 0x567b2c in eden_en.exe) emits, after `LC` (vt+64,
/// line count) and `L0`, for each line n: `L1_n` = vt+28 (voice), `L2_n` =
/// vt+40 (speaker), `L3_n` = vt+16 (text), `L4_n` = vt+52 `sub_4F0030`.
#[derive(Clone, PartialEq, Eq)]
pub struct EdenHistoryLine {
    pub voice: String,
    pub speaker: String,
    pub text: String,
    /// `L4_n`: vertical position where the line starts in the novel window.
    /// CBacklog+12 `sub_4F0B20` stores it through CLog+48 from scene+4804,
    /// which message execute `sub_4D35D0` sets from the window's vt+120
    /// (`sub_427930`). The input handler `sub_4D2660` hit-tests the mouse y
    /// against vt+52 to pick the clicked line.
    pub top_y: i32,
}

struct Fields<'a>(BTreeMap<&'a str, &'a str>);
impl<'a> Fields<'a> {
    fn new(fields: &'a [EdenSaveField]) -> Result<Self, CoreError> {
        let map: BTreeMap<_, _> = fields
            .iter()
            .map(|field| (field.name.as_str(), field.value.as_str()))
            .collect();
        if map.len() != fields.len() {
            return Err(unsupported());
        }
        Ok(Self(map))
    }
    fn take(&mut self, name: &str) -> Result<&'a str, CoreError> {
        self.0.remove(name).ok_or_else(unsupported)
    }
    fn number<T: std::str::FromStr>(&mut self, name: &str) -> Result<T, CoreError> {
        self.take(name)?.parse().map_err(|_| unsupported())
    }
    fn expect(&mut self, name: &str, value: &str) -> Result<(), CoreError> {
        if self.take(name)? != value {
            return Err(unsupported());
        }
        Ok(())
    }
    fn finish(self) -> Result<(), CoreError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(unsupported())
        }
    }
}

pub(super) fn unsupported() -> CoreError {
    invalid(
        "STATE",
        "native eden checkpoint contains unsupported or inconsistent state",
    )
}

impl EdenSave {
    /// Project the supported native message boundary. This does not execute scripts.
    pub fn checkpoint(&self) -> Result<EdenCheckpoint, CoreError> {
        if self.route != [0; 4] || self.backlog.is_empty() {
            return Err(unsupported());
        }
        let mut fields = Fields::new(&self.variables)?;
        for name in ["angBG", "angFG"] {
            fields.expect(name, "0000000000000000")?;
        }
        for name in [
            "bgH",
            "bgS",
            "bgV",
            "stH",
            "stS",
            "stV",
            "stage_FGX",
            "stage_FGY",
            "rotBG_dest.x",
            "rotBG_dest.y",
            "rotBG_source.x",
            "rotBG_source.y",
            "rotFG_dest.x",
            "rotFG_dest.y",
            "rotFG_source.x",
            "rotFG_source.y",
            "tr_Method",
        ] {
            fields.expect(name, "0")?;
        }
        for name in ["eff_Count", "eff_Param", "eff_Speed", "frameParam"] {
            fields.expect(name, "-1")?;
        }
        if let Some(method) = fields.0.remove("eff_Method")
            && method != "end"
        {
            return Err(unsupported());
        }
        fields.expect("font_Color", "16777215")?;
        fields.expect("font_FontSize", "22")?;
        fields.expect("font_RubySize", "10")?;
        let history = self
            .backlog
            .iter()
            .map(|record| EdenHistoryMessage::parse(record))
            .collect::<Result<Vec<_>, _>>()?;
        let last = history.last().ok_or_else(unsupported)?;
        let mut checkpoint = EdenCheckpoint {
            script: script_name(fields.take("script_Filename")?)?,
            next_line: fields.number("script_Pointer")?,
            message_id: fields.number("script_ID")?,
            background: resource_name(fields.take("stage_BGFilename")?)?,
            background_position: [fields.number("stage_BGX")?, fields.number("stage_BGY")?],
            panel_mode: fields.number("panel_Mode")?,
            // The native variable table omits the custom panel filename.
            panel_resource: last.panel_resource.clone(),
            bgm: resource_name(fields.take("bgm_Filename")?)?,
            bgm_volume: fields.number("bgm_Volume")?,
            transition_ticks: fields.number("tr_Param")?,
            history: Vec::new(),
        };
        fields.finish()?;
        if checkpoint.next_line == 0
            || i32::try_from(checkpoint.next_line).is_err()
            || i32::try_from(checkpoint.message_id).is_err()
            || checkpoint.bgm_volume > 100
            || checkpoint.script != last.script
            || checkpoint.next_line != last.next_line
            || checkpoint.message_id != last.message_id
            || checkpoint.background != last.background
            || checkpoint.background_position != last.background_position
            || checkpoint.panel_mode != last.panel_mode
            || checkpoint.bgm != last.bgm
            || checkpoint.bgm_volume != last.bgm_volume
            || checkpoint.transition_ticks != last.transition_ticks
            || last.sound_effects.iter().any(|name| !name.is_empty())
        {
            return Err(unsupported());
        }
        checkpoint.history = history;
        Ok(checkpoint)
    }
}

/// Upper bound on `LC`; the seven real eden_en slots hold at most 6 lines.
pub(super) const EDEN_PAGE_LINE_LIMIT: usize = 64;

impl EdenHistoryMessage {
    fn parse(record: &[EdenSaveField]) -> Result<Self, CoreError> {
        let mut fields = Fields::new(record)?;
        let panel_mode: u32 = fields.number("CP1")?;
        let line_count: usize = fields.number("LC")?;
        // sub_4F1920 takes the page path exactly when the window mode is 2.
        if (panel_mode == 2) != (line_count > 0) || line_count > EDEN_PAGE_LINE_LIMIT {
            return Err(unsupported());
        }
        let (voice, speaker, text, page) = if line_count == 0 {
            (
                fields.take("L1")?.into(),
                fields.take("L2")?.into(),
                fields.take("L3")?.into(),
                Vec::new(),
            )
        } else {
            let page = (0..line_count)
                .map(|line| {
                    Ok(EdenHistoryLine {
                        voice: fields.take(&format!("L1_{line}"))?.into(),
                        speaker: fields.take(&format!("L2_{line}"))?.into(),
                        text: fields.take(&format!("L3_{line}"))?.into(),
                        top_y: fields.number(&format!("L4_{line}"))?,
                    })
                })
                .collect::<Result<Vec<_>, CoreError>>()?;
            (String::new(), String::new(), String::new(), page)
        };
        fields.expect("CF1", "16777215")?;
        fields.expect("CF2", "22")?;
        fields.expect("CF3", "10")?;
        for name in ["CT6", "CT7", "bgH", "bgS", "bgV", "stH", "stS", "stV"] {
            fields.expect(name, "0")?;
        }
        for name in [
            "CT1", "Frm", "CO0", "CO1", "CO2", "Cs0", "Cs1", "Cs2", "Ce2", "Ct2",
        ] {
            fields.expect(name, "")?;
        }
        for name in ["FrP", "Ce3", "Ce4", "Ce5"] {
            fields.expect(name, "-1")?;
        }
        let message_id = fields.number("L0")?;
        let state_id: i64 = fields.number("CS3")?;
        let state_message_id = if state_id == message_id {
            None
        } else if message_id == -1
            && i32::try_from(state_id).is_ok()
            && !page.is_empty()
            && page.iter().all(|line| {
                line.voice.is_empty() && line.speaker.is_empty() && line.text.is_empty()
            })
        {
            Some(state_id)
        } else {
            return Err(unsupported());
        };
        let message = Self {
            script: script_name(fields.take("CS1")?)?,
            next_line: fields.number("CS2")?,
            message_id,
            state_message_id,
            voice,
            speaker,
            text,
            page,
            background: resource_name(fields.take("CT2")?)?,
            background_position: [fields.number("CT3")?, fields.number("CT4")?],
            panel_mode,
            panel_resource: resource_name(fields.take("CP2")?)?,
            bgm: resource_name(fields.take("CB1")?)?,
            bgm_volume: fields.number("CB1v")?,
            sound_effects: [
                resource_name(fields.take("CE1")?)?,
                resource_name(fields.take("CE2")?)?,
            ],
            stand_count: fields.number("CT5")?,
            transition_ticks: fields.number("Ct3")?,
            fire_effect: resource_name(fields.take("Ce1")?)?,
            transition_method: fields.number("Ct1")?,
        };
        fields.finish()?;
        if message.next_line == 0
            || i32::try_from(message.next_line).is_err()
            || i32::try_from(message.message_id).is_err()
            || i32::try_from(message.transition_ticks).is_err()
            || message.bgm_volume > 100
            || !matches!(message.panel_mode, 0..=3)
            || !matches!(message.transition_method, 0 | 1)
        {
            return Err(unsupported());
        }
        Ok(message)
    }
}

fn script_name(name: &str) -> Result<String, CoreError> {
    let value = resource_name(name)?;
    if !value.to_ascii_lowercase().ends_with(".sc") {
        return Err(unsupported());
    }
    Ok(value)
}

fn resource_name(name: &str) -> Result<String, CoreError> {
    if name.len() > 256 || name.contains(['/', '\\', ':', '\0']) || matches!(name, "." | "..") {
        return Err(unsupported());
    }
    Ok(name.into())
}
