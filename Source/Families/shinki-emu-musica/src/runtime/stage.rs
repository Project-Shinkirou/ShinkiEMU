use super::effects::validate_scene_filename;
use super::*;

#[cfg(test)]
mod tests;

pub(super) fn execute_stage(
    command: &ScCommand,
    state: &mut MusicaRuntimeState,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    let tokens = command.tokens().map_err(|_| MusicaRuntimeError::Operand)?;
    // Native scripts may end the operand list with one empty field.
    let tokens = if tokens.last().is_some_and(String::is_empty) {
        &tokens[..tokens.len() - 1]
    } else {
        &tokens
    };
    if !(4..=26).contains(&tokens.len()) || tokens.last().is_some_and(String::is_empty) {
        return Err(MusicaRuntimeError::Operand);
    }
    let foreground = parse_foreground_spec(&tokens[0])?;
    let mut cursor = 1;
    // Native reference coordinates: both tokens must be dot-free and carry a
    // leading decimal integer (native `sscanf("%d")` semantics).
    let reference_position =
        if tokens.len() >= 6 && !tokens[1].contains('.') && !tokens[2].contains('.') {
            match (scan_int(&tokens[1]), scan_int(&tokens[2])) {
                (Some(x), Some(y)) => {
                    cursor = 3;
                    Some([x, y])
                }
                _ => None,
            }
        } else {
            None
        };
    if cursor + 3 > tokens.len() || !(tokens.len() - cursor - 3).is_multiple_of(2) {
        return Err(MusicaRuntimeError::Operand);
    }
    let x = scan_int(&tokens[cursor + 1]).unwrap_or(0);
    let y = scan_int(&tokens[cursor + 2]).unwrap_or(0);
    let background = resource("bg", &tokens[cursor])?.map(|resource_uri| MusicaStageLayer {
        resource_uri,
        x,
        y,
    });
    cursor += 3;
    let mut stands = Vec::new();
    while cursor < tokens.len() {
        let resource_uri = resource("st", &tokens[cursor])?.ok_or(MusicaRuntimeError::Operand)?;
        // Native stand position spec is `pos[,offs]` (separator 0x4F1374 = ','),
        // each half parsed with leading-integer scan defaulting to zero.
        let mut spec = tokens[cursor + 1].split(',');
        let position = scan_int(spec.next().ok_or(MusicaRuntimeError::Operand)?).unwrap_or(0);
        let resource_parameter = spec
            .next()
            .map(|value| scan_int(value).unwrap_or(0))
            .unwrap_or(0);
        if spec.next().is_some() {
            return Err(MusicaRuntimeError::Operand);
        }
        stands.push(MusicaStandLayer {
            resource_uri,
            position,
            resource_parameter,
        });
        cursor += 2;
    }
    let stage = MusicaStageCommand {
        foreground,
        reference_position,
        background,
        stands,
        transition: state.transition.clone(),
    };
    validate_stage_state(Some(&stage))?;
    next_effect_sequence(state)?;
    state.native_foreground = tokens[0].clone();
    state.native_stand_slots = 10;
    let previous = state.stage.clone();
    state.characters.retain(|_, character| {
        let retained = character.keep_once || character.pending_stage;
        character.keep_once = false;
        character.pending_stage = false;
        retained
    });
    state.stage = Some(stage.clone());
    state.axis_scroll = None;
    state.linear_scroll = None;
    state.scroll_xf = None;
    state.transition_active = begin_transition(previous.as_ref(), &stage, &state.transition);
    Ok(Some(MusicaVmEvent::Stage(stage)))
}

/// Native composite foreground grammar (sub_42E900): the operand splits on
/// `:` into the prefix layer (head) and a slot list; the slot list splits on
/// `+` (tail becomes the `plus` layer) and then on up to two `,` (main,
/// second, third). `*` and empty fragments hide their layer.
pub(super) fn parse_foreground_spec(
    spec: &str,
) -> Result<MusicaForegroundLayers, MusicaRuntimeError> {
    let (prefix, rest) = match spec.split_once(':') {
        Some((head, tail)) => (Some(head), tail),
        None => (None, spec),
    };
    let (main_part, plus) = match rest.split_once('+') {
        Some((head, tail)) => (head, Some(tail)),
        None => (rest, None),
    };
    let mut commas = main_part.splitn(3, ',');
    let main = commas.next();
    let second = commas.next();
    let third = commas.next();
    Ok(MusicaForegroundLayers {
        main: resource("bg", main.unwrap_or(""))?,
        second: resource("bg", second.unwrap_or(""))?,
        third: resource("bg", third.unwrap_or(""))?,
        plus: resource("bg", plus.unwrap_or(""))?,
        prefix: resource("bg", prefix.unwrap_or(""))?,
    })
}

/// Native numeric scan (`sscanf("%d")`): an optional sign followed by decimal
/// digits from the start of the token; anything else fails.
pub(super) fn scan_int(value: &str) -> Option<i32> {
    let bytes = value.as_bytes();
    let mut index = 0;
    if index < bytes.len() && (bytes[index] == b'-' || bytes[index] == b'+') {
        index += 1;
    }
    let digits = bytes[index..]
        .iter()
        .position(|byte| !byte.is_ascii_digit())
        .unwrap_or(bytes.len() - index);
    if digits == 0 {
        return None;
    }
    value[..index + digits].parse().ok()
}

/// Native transitions arm once per stage submission (sub_45AC10). Mode 0
/// crossfades for `25_600 / param` ms (phase 256); modes 1 and 18 reveal the
/// new stage through the rule mask for `51_200 / param` and `25_600 / param`
/// ms respectively (native phase maxima 512 and 256). Other methods complete
/// immediately until their native compositing is implemented.
fn begin_transition(
    previous: Option<&MusicaStageCommand>,
    stage: &MusicaStageCommand,
    pending: &MusicaTransitionState,
) -> Option<MusicaTransitionActiveState> {
    let previous = previous?;
    if pending.param == 0 || previous == stage {
        return None;
    }
    let phase_max: u64 = match pending.mode {
        0 => 256,
        1 if pending.resource.is_some() => 512,
        18 if pending.resource.is_some() => 256,
        _ => {
            tracing::warn!(
                event = "shinki.emu.musica.transition.unsupported",
                method = pending.mode,
                "native transition method completes instantly until implemented"
            );
            return None;
        }
    };
    let duration_ns = phase_max * 100_000_000 / u64::from(pending.param);
    Some(MusicaTransitionActiveState {
        method: pending.mode,
        param: pending.param,
        elapsed_ns: 0,
        duration_ns,
        previous: Box::new(previous.clone()),
    })
}

pub(super) fn resource(role: &str, name: &str) -> Result<Option<String>, MusicaRuntimeError> {
    if name.is_empty() || name == "*" {
        return Ok(None);
    }
    validate_scene_filename(name)?;
    Ok(Some(format!("musica:/{role}/{name}")))
}

pub(super) fn validate_stage_state(
    stage: Option<&MusicaStageCommand>,
) -> Result<(), MusicaRuntimeError> {
    let Some(stage) = stage else {
        return Ok(());
    };
    if stage.stands.len() > 10 {
        return Err(MusicaRuntimeError::State);
    }
    for uri in stage.foreground.uris_in_order() {
        validate_uri(uri, "musica:/bg/")?;
    }
    if let Some(background) = &stage.background {
        validate_uri(&background.resource_uri, "musica:/bg/")?;
    }
    for stand in &stage.stands {
        validate_uri(&stand.resource_uri, "musica:/st/")?;
    }
    if let Some(name) = &stage.transition.resource {
        validate_scene_filename(name)?;
    }
    Ok(())
}

pub(super) fn validate_uri(uri: &str, prefix: &str) -> Result<(), MusicaRuntimeError> {
    let name = uri.strip_prefix(prefix).ok_or(MusicaRuntimeError::State)?;
    if name == "*" {
        return Err(MusicaRuntimeError::State);
    }
    validate_scene_filename(name)
}
