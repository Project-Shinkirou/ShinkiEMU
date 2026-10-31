use super::*;

fn field(fields: &mut Vec<EdenSaveField>, name: impl Into<String>, value: impl ToString) {
    fields.push(EdenSaveField {
        name: name.into(),
        value: value.to_string(),
    });
}

impl EdenSave {
    /// Build the native variable map and CLog records from a live message boundary.
    /// Every synthetic field is a constructor value from the original eden/perseus
    /// state writers; unsupported effects are rejected before reaching this point.
    pub fn from_checkpoint(
        checkpoint: &EdenCheckpoint,
        edition: EdenEdition,
        encoding: EdenSaveEncoding,
        prefix: u32,
        comment: String,
    ) -> Result<Self, CoreError> {
        let last = checkpoint
            .history
            .last()
            .ok_or_else(checkpoint::unsupported)?;
        if last.script != checkpoint.script
            || last.next_line != checkpoint.next_line
            || last.message_id != checkpoint.message_id
            || last.background != checkpoint.background
            || last.background_position != checkpoint.background_position
            || last.foreground != checkpoint.foreground
            || last.foreground_position != checkpoint.foreground_position
            || last.stands != checkpoint.stands
            || last.panel_mode != checkpoint.panel_mode
            || last.bgm != checkpoint.bgm
            || last.bgm_volume != checkpoint.bgm_volume
            || last.transition_ticks != checkpoint.transition_ticks
        {
            return Err(checkpoint::unsupported());
        }
        let mut variables = Vec::new();
        field(&mut variables, "script_Filename", &checkpoint.script);
        field(&mut variables, "script_Pointer", checkpoint.next_line);
        field(&mut variables, "script_ID", checkpoint.message_id);
        field(&mut variables, "font_Color", 16777215);
        let (font, ruby) = checkpoint::font_sizes(edition);
        field(&mut variables, "font_FontSize", font);
        field(&mut variables, "font_RubySize", ruby);
        field(&mut variables, "panel_Mode", checkpoint.panel_mode);
        field(&mut variables, "stage_FGFilename", &checkpoint.foreground);
        field(&mut variables, "stage_BGFilename", &checkpoint.background);
        field(
            &mut variables,
            "stage_BGX",
            checkpoint.background_position[0],
        );
        field(
            &mut variables,
            "stage_BGY",
            checkpoint.background_position[1],
        );
        field(
            &mut variables,
            "stage_FGX",
            checkpoint.foreground_position[0],
        );
        field(
            &mut variables,
            "stage_FGY",
            checkpoint.foreground_position[1],
        );
        for stand in &checkpoint.stands {
            field(
                &mut variables,
                format!("stage_StandFilename{}", stand.slot),
                &stand.resource,
            );
            field(
                &mut variables,
                format!("stage_StandPos{}", stand.slot),
                stand.position,
            );
            field(
                &mut variables,
                format!("stage_StandOffs{}", stand.slot),
                stand.offset,
            );
        }
        for name in [
            "bgH",
            "bgS",
            "bgV",
            "stH",
            "stS",
            "stV",
            "tr_Method",
            "rotBG_dest.x",
            "rotBG_dest.y",
            "rotBG_source.x",
            "rotBG_source.y",
            "rotFG_dest.x",
            "rotFG_dest.y",
            "rotFG_source.x",
            "rotFG_source.y",
        ] {
            field(&mut variables, name, 0);
        }
        for name in ["eff_Count", "eff_Speed", "eff_Param", "frameParam"] {
            field(&mut variables, name, -1);
        }
        if edition == EdenEdition::Perseus {
            for name in ["eff2_Count", "eff2_Speed", "eff2_Param"] {
                field(&mut variables, name, -1);
            }
        }
        for name in ["angBG", "angFG"] {
            field(&mut variables, name, "0000000000000000");
        }
        field(&mut variables, "bgm_Filename", &checkpoint.bgm);
        field(&mut variables, "bgm_Volume", checkpoint.bgm_volume);
        field(&mut variables, "tr_Param", checkpoint.transition_ticks);
        let backlog = checkpoint
            .history
            .iter()
            .map(|message| {
                let mut record = Vec::new();
                field(&mut record, "CS1", &message.script);
                field(&mut record, "CS2", message.next_line);
                field(
                    &mut record,
                    "CS3",
                    message.state_message_id.unwrap_or(message.message_id),
                );
                field(&mut record, "CF1", 16777215);
                field(&mut record, "CF2", font);
                field(&mut record, "CF3", ruby);
                field(&mut record, "CP1", message.panel_mode);
                field(&mut record, "CP2", &message.panel_resource);
                field(&mut record, "Ct1", message.transition_method);
                field(&mut record, "Ct2", &message.transition_resource);
                field(&mut record, "Ct3", message.transition_ticks);
                field(&mut record, "CT1", &message.foreground);
                field(&mut record, "CT2", &message.background);
                field(&mut record, "CT3", message.background_position[0]);
                field(&mut record, "CT4", message.background_position[1]);
                field(&mut record, "CT5", message.stand_count);
                for stand in &message.stands {
                    field(&mut record, format!("CT6_{}", stand.slot), &stand.resource);
                    field(&mut record, format!("CT7_{}", stand.slot), stand.position);
                    field(&mut record, format!("CT7Y_{}", stand.slot), stand.offset);
                }
                field(&mut record, "CT6", message.foreground_position[0]);
                field(&mut record, "CT7", message.foreground_position[1]);
                for name in ["Frm", "CO0", "CO1", "CO2", "Cs0", "Cs1", "Cs2"] {
                    field(&mut record, name, "");
                }
                field(&mut record, "FrP", -1);
                field(&mut record, "Ce2", &message.effect_resource);
                field(&mut record, "Ce3", message.effect_count);
                field(&mut record, "Ce4", message.effect_speed);
                field(&mut record, "Ce5", message.effect_param);
                if edition == EdenEdition::Perseus {
                    for name in ["CE3", "CE4", "Ce21", "Ce22"] {
                        field(&mut record, name, "");
                    }
                    for name in ["Ce23", "Ce24", "Ce25"] {
                        field(&mut record, name, -1);
                    }
                }
                for name in ["bgH", "bgS", "bgV", "stH", "stS", "stV"] {
                    field(&mut record, name, 0);
                }
                field(&mut record, "CB1", &message.bgm);
                field(&mut record, "CB1v", message.bgm_volume);
                field(&mut record, "CE1", &message.sound_effects[0]);
                field(&mut record, "CE2", &message.sound_effects[1]);
                field(&mut record, "Ce1", &message.fire_effect);
                field(&mut record, "L0", message.message_id);
                field(&mut record, "LC", message.page.len());
                if message.panel_mode == 2 {
                    for (index, line) in message.page.iter().enumerate() {
                        field(&mut record, format!("L1_{index}"), &line.voice);
                        field(&mut record, format!("L2_{index}"), &line.speaker);
                        field(&mut record, format!("L3_{index}"), &line.text);
                        field(&mut record, format!("L4_{index}"), line.top_y);
                    }
                } else {
                    field(&mut record, "L1", &message.voice);
                    field(&mut record, "L2", &message.speaker);
                    field(&mut record, "L3", &message.text);
                }
                record
            })
            .collect();
        let save = Self {
            edition,
            encoding,
            prefix,
            comment,
            route: [0; 4],
            variables,
            script_variables: checkpoint
                .script_variables
                .iter()
                .map(|(name, value)| (name.clone(), value.to_string()))
                .collect(),
            backlog,
        };
        save.checkpoint()?;
        Ok(save)
    }
}
