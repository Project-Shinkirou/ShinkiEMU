use super::*;
pub(crate) fn config_control_at(
    x: i32,
    y: i32,
    character_count: usize,
) -> Option<MusicaConfigControl> {
    for index in 0..character_count {
        let [left, top, width, height] = voice_control_rect(character_count, index);
        if (left..left + width).contains(&x) && (top..top + height).contains(&y) {
            return Some(MusicaConfigControl::ToggleCharacterVoice(index));
        }
    }
    let slider_value = |track_left: i32| {
        let position = (x - track_left - 11).clamp(0, 200);
        u8::try_from(position * 100 / 200).expect("clamped config slider fits u8")
    };
    if (40..260).contains(&x) {
        return match y {
            152..192 => Some(MusicaConfigControl::MessageSpeedUnread(slider_value(40))),
            228..268 => Some(MusicaConfigControl::MessageSpeedRead(slider_value(40))),
            304..340 => Some(MusicaConfigControl::MessageSpeedAutoPlay(slider_value(40))),
            _ => config_non_slider_control_at(x, y),
        };
    }
    if (576..796).contains(&x) {
        return match y {
            148..188 => Some(MusicaConfigControl::BgmVolume(slider_value(576))),
            224..264 => Some(MusicaConfigControl::VoiceVolume(slider_value(576))),
            300..336 => Some(MusicaConfigControl::SeVolume(slider_value(576))),
            _ => config_non_slider_control_at(x, y),
        };
    }
    config_non_slider_control_at(x, y)
}

fn config_non_slider_control_at(x: i32, y: i32) -> Option<MusicaConfigControl> {
    let hit = |left, top, right, bottom| (left..right).contains(&x) && (top..bottom).contains(&y);
    let control = if hit(248, 492, 276, 512) {
        MusicaConfigControl::FontPrevious
    } else if hit(248, 524, 276, 548) {
        MusicaConfigControl::FontNext
    } else if hit(36, 592, 132, 616) {
        MusicaConfigControl::PreferredPlayMode(MusicaPlayMode::Auto)
    } else if hit(148, 592, 268, 616) {
        MusicaConfigControl::PreferredPlayMode(MusicaPlayMode::Skip)
    } else if hit(312, 120, 456, 152) {
        MusicaConfigControl::Fullscreen(true)
    } else if hit(312, 164, 456, 196) {
        MusicaConfigControl::Fullscreen(false)
    } else if hit(312, 248, 544, 280) {
        MusicaConfigControl::ToggleScreenEffect
    } else if hit(312, 292, 544, 320) {
        MusicaConfigControl::ToggleTextShadow
    } else if hit(312, 336, 544, 364) {
        MusicaConfigControl::ToggleAnimation
    } else if hit(312, 424, 512, 472) {
        MusicaConfigControl::ToggleBacklogVoicePlayback
    } else if hit(312, 476, 512, 524) {
        MusicaConfigControl::ToggleStopVoiceAtNextMessage
    } else if hit(312, 572, 512, 620) {
        MusicaConfigControl::ToggleProgressInBackground
    } else if hit(680, 120, 740, 144) {
        MusicaConfigControl::ToggleBgmMute
    } else if hit(680, 196, 740, 220) {
        MusicaConfigControl::ToggleVoiceMute
    } else if hit(680, 268, 740, 292) {
        MusicaConfigControl::ToggleSeMute
    } else if hit(744, 120, 780, 144) {
        MusicaConfigControl::TestAudio(MusicaConfigAudioBus::Bgm)
    } else if hit(744, 196, 780, 220) {
        MusicaConfigControl::TestAudio(MusicaConfigAudioBus::Voice)
    } else if hit(744, 268, 780, 292) {
        MusicaConfigControl::TestAudio(MusicaConfigAudioBus::Se)
    } else if hit(592, 600, 648, 640) {
        MusicaConfigControl::Apply
    } else if hit(701, 600, 775, 640) {
        MusicaConfigControl::Cancel
    } else {
        return None;
    };
    Some(control)
}

pub(crate) fn voice_control_rect(count: usize, index: usize) -> [i32; 4] {
    let rows = count.clamp(1, 8);
    let columns = count.div_ceil(rows).max(1);
    let width = 220 / columns as i32;
    [
        570 + index.div_euclid(rows) as i32 * width,
        420 + (index % rows) as i32 * 21,
        width,
        21,
    ]
}
