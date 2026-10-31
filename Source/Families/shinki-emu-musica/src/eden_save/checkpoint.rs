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
    pub foreground: String,
    pub foreground_position: [i32; 2],
    pub stands: Vec<EdenStand>,
    pub script_variables: BTreeMap<String, i64>,
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
    #[serde(skip)]
    pub state_message_id: Option<i64>,
    /// Plain `L1`/`L2`/`L3`; empty for a novel page (`CP1=2`), whose lines are in `page`.
    pub voice: String,
    pub speaker: String,
    pub text: String,
    /// Novel-mode (`CP1=2`) page lines `L1_n..L4_n`, `LC` of them; empty otherwise.
    #[serde(skip)]
    pub page: Vec<EdenHistoryLine>,
    pub background: String,
    pub background_position: [i32; 2],
    #[serde(skip)]
    pub foreground: String,
    #[serde(skip)]
    pub foreground_position: [i32; 2],
    #[serde(skip)]
    pub stands: Vec<EdenStand>,
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
    pub effect_resource: String,
    pub effect_count: i32,
    pub effect_speed: i32,
    pub effect_param: i32,
    /// Native transition method (Ct1): 0 or 1.
    pub transition_method: i32,
    pub transition_resource: String,
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
#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdenStand {
    pub slot: u32,
    pub resource: String,
    pub position: i32,
    pub offset: i32,
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
        self.0.remove(name).ok_or_else(|| {
            invalid(
                "STATE",
                &format!("native checkpoint field {name} is missing"),
            )
        })
    }
    fn number<T: std::str::FromStr>(&mut self, name: &str) -> Result<T, CoreError> {
        self.take(name)?.parse().map_err(|_| {
            invalid(
                "STATE",
                &format!("native checkpoint field {name} is not numeric"),
            )
        })
    }
    fn expect(&mut self, name: &str, value: &str) -> Result<(), CoreError> {
        if self.take(name)? != value {
            return Err(invalid(
                "STATE",
                &format!("native checkpoint field {name} has an unsupported value"),
            ));
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
        let (font, ruby) = font_sizes(self.edition);
        fields.expect("font_FontSize", font)?;
        fields.expect("font_RubySize", ruby)?;
        if self.edition == EdenEdition::Perseus {
            for name in ["eff2_Count", "eff2_Param", "eff2_Speed"] {
                fields.expect(name, "-1")?;
            }
        }
        let history = self
            .backlog
            .iter()
            .enumerate()
            .map(|(index, record)| {
                EdenHistoryMessage::parse(record, self.edition).map_err(|cause| {
                    invalid(
                        "STATE",
                        &format!("native history record {index}: {cause:?}"),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let last = history.last().ok_or_else(unsupported)?;
        let mut checkpoint = EdenCheckpoint {
            script: script_name(fields.take("script_Filename")?)?,
            next_line: fields.number("script_Pointer")?,
            message_id: fields.number("script_ID")?,
            background: resource_name(fields.0.remove("stage_BGFilename").unwrap_or(""))?,
            background_position: [fields.number("stage_BGX")?, fields.number("stage_BGY")?],
            foreground: fields.0.remove("stage_FGFilename").unwrap_or("").into(),
            foreground_position: [fields.number("stage_FGX")?, fields.number("stage_FGY")?],
            stands: parse_stands(&mut fields, 10, true)?,
            script_variables: self
                .script_variables
                .iter()
                .map(|(key, value)| {
                    Ok((
                        key.clone(),
                        value.parse::<i64>().map_err(|_| unsupported())?,
                    ))
                })
                .collect::<Result<_, CoreError>>()?,
            panel_mode: fields.number("panel_Mode")?,
            // The native variable table omits the custom panel filename.
            panel_resource: last.panel_resource.clone(),
            // Native variable writer omits empty strings; the constructor clears BGM.
            bgm: resource_name(fields.0.remove("bgm_Filename").unwrap_or(""))?,
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
            || checkpoint.foreground != last.foreground
            || checkpoint.foreground_position != last.foreground_position
            || checkpoint.stands != last.stands
            || checkpoint.panel_mode != last.panel_mode
            || checkpoint.bgm != last.bgm
            || checkpoint.bgm_volume != last.bgm_volume
            || checkpoint.transition_ticks != last.transition_ticks
            || last.sound_effects.iter().any(|name| !name.is_empty())
            || !matches!(last.fire_effect.as_str(), "" | "end")
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
    fn parse(record: &[EdenSaveField], edition: EdenEdition) -> Result<Self, CoreError> {
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
        let (font, ruby) = font_sizes(edition);
        fields.expect("CF2", font)?;
        fields.expect("CF3", ruby)?;
        for name in ["bgH", "bgS", "bgV", "stH", "stS", "stV"] {
            fields.expect(name, "0")?;
        }
        for name in ["Frm", "CO0", "CO1", "CO2", "Cs0", "Cs1", "Cs2"] {
            fields.expect(name, "")?;
        }
        fields.expect("FrP", "-1")?;
        if edition == EdenEdition::Perseus {
            for name in ["CE3", "CE4", "Ce21", "Ce22"] {
                fields.expect(name, "")?;
            }
            for name in ["Ce23", "Ce24", "Ce25"] {
                fields.expect(name, "-1")?;
            }
        }
        let stand_count = fields.number("CT5")?;
        let stands = parse_stands(&mut fields, stand_count, false)?;
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
            foreground: fields.take("CT1")?.into(),
            foreground_position: [fields.number("CT6")?, fields.number("CT7")?],
            stands,
            panel_mode,
            panel_resource: resource_name(fields.take("CP2")?)?,
            bgm: resource_name(fields.take("CB1")?)?,
            bgm_volume: fields.number("CB1v")?,
            sound_effects: [
                resource_name(fields.take("CE1")?)?,
                resource_name(fields.take("CE2")?)?,
            ],
            stand_count,
            transition_ticks: fields.number("Ct3")?,
            fire_effect: resource_name(fields.take("Ce1")?)?,
            effect_resource: resource_name(fields.take("Ce2")?)?,
            effect_count: fields.number("Ce3")?,
            effect_speed: fields.number("Ce4")?,
            effect_param: fields.number("Ce5")?,
            transition_method: fields.number("Ct1")?,
            transition_resource: resource_name(fields.take("Ct2")?)?,
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

pub(super) fn font_sizes(edition: EdenEdition) -> (&'static str, &'static str) {
    if edition == EdenEdition::Perseus {
        ("26", "12")
    } else {
        ("22", "10")
    }
}

fn parse_stands(
    fields: &mut Fields<'_>,
    count: u32,
    variables: bool,
) -> Result<Vec<EdenStand>, CoreError> {
    if count > 10 {
        return Err(unsupported());
    }
    let mut stands = Vec::new();
    for slot in 0..count {
        let (name, position, offset) = if variables {
            (
                format!("stage_StandFilename{slot}"),
                format!("stage_StandPos{slot}"),
                format!("stage_StandOffs{slot}"),
            )
        } else {
            (
                format!("CT6_{slot}"),
                format!("CT7_{slot}"),
                format!("CT7Y_{slot}"),
            )
        };
        if let Some(resource) = fields.0.remove(name.as_str()) {
            if resource.is_empty() {
                return Err(unsupported());
            }
            stands.push(EdenStand {
                slot,
                resource: resource_name(resource)?,
                position: fields.number(&position)?,
                offset: fields.number(&offset)?,
            });
        }
    }
    Ok(stands)
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
