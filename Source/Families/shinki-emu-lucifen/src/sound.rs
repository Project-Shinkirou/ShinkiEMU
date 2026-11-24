use std::{io, sync::atomic::AtomicBool};

const OUTPUT_RATE: f64 = 48_000.0;
const MAX_DECODED_FRAMES: usize = 48_000 * 300;
const CHANNEL_HANDLE: i32 = 0x4000_0000;
const MUSIC_HANDLE: i32 = CHANNEL_HANDLE | 256;

struct Fade {
    elapsed: u64,
    duration: u64,
    start: f32,
    stop_when_done: bool,
}

impl Fade {
    fn gain(&self) -> f32 {
        let progress = self.elapsed as f32 / self.duration as f32;
        let end = if self.stop_when_done { 0.0 } else { 1.0 };
        self.start + (end - self.start) * progress
    }
}

struct Channel {
    clip: Option<kira::sound::static_sound::StaticSoundData>,
    following: Option<kira::sound::static_sound::StaticSoundData>,
    position: f64,
    volume: i32,
    gain: i32,
    pan: i32,
    speed: f64,
    kind: u8,
    playing: bool,
    repeat: bool,
    finished: bool,
    fade: Option<Fade>,
    delay: u64,
}

impl Default for Channel {
    fn default() -> Self {
        Self {
            clip: None,
            following: None,
            position: 0.0,
            volume: 100,
            gain: 256,
            pan: 0,
            speed: 1.0,
            kind: 1,
            playing: false,
            repeat: false,
            finished: false,
            fade: None,
            delay: 0,
        }
    }
}

impl Channel {
    fn start(&mut self, repeat: bool) {
        self.position = 0.0;
        self.repeat = repeat;
        self.playing = self.clip.is_some();
        self.finished = false;
        self.fade = None;
    }

    fn stop(&mut self) {
        self.playing = false;
        self.position = 0.0;
        self.fade = None;
    }
}

#[derive(Default)]
pub(crate) struct Channels {
    slots: Vec<Channel>,
    music: Channel,
    music_slot: bool,
    fading_music: Option<Channel>,
    crossfade: bool,
    music_identity: Option<(String, Option<String>)>,
    previous_ms: Option<u32>,
}

pub(crate) struct Crossfade {
    pub(crate) duration: u32,
    pub(crate) overlap: u32,
    pub(crate) volume: i32,
}

pub(crate) struct MusicSource<'a> {
    pub(crate) name: String,
    pub(crate) bytes: &'a [u8],
    pub(crate) intro: Option<(String, &'a [u8])>,
}

impl MusicSource<'_> {
    fn open(self, stop: &AtomicBool) -> Option<(Channel, (String, Option<String>))> {
        let main = shinki_emu_sdk::decode_audio(self.bytes, MAX_DECODED_FRAMES, stop).ok()?;
        if let Some((name, bytes)) = self.intro {
            let intro = shinki_emu_sdk::decode_audio(bytes, MAX_DECODED_FRAMES, stop).ok()?;
            Some((
                Channel {
                    clip: Some(intro),
                    following: Some(main),
                    ..Channel::default()
                },
                (self.name, Some(name)),
            ))
        } else {
            Some((
                Channel {
                    clip: Some(main),
                    ..Channel::default()
                },
                (self.name, None),
            ))
        }
    }
}

impl Channels {
    pub(crate) fn resize(&mut self, count: i32) -> i32 {
        let Ok(count) = usize::try_from(count) else {
            return 1;
        };
        if !(1..=256).contains(&count) {
            return 1;
        }
        self.slots.resize_with(count, Channel::default);
        0
    }

    pub(crate) fn valid(&self, id: i32) -> bool {
        (id as usize) < self.slots.len()
    }

    pub(crate) fn volume(&mut self, id: i32, volume: i32, gain: i32) -> i32 {
        let Some(channel) = self.slots.get_mut(id as usize) else {
            return 1;
        };
        if volume >= 0 {
            channel.volume = volume.min(100);
        }
        if gain >= 0 {
            channel.gain = gain.min(256);
        }
        0
    }

    pub(crate) fn set_kind(&mut self, id: i32, kind: u8) -> i32 {
        let Some(channel) = self.slots.get_mut(id as usize) else {
            return 1;
        };
        if kind & 7 == 0 {
            return 2;
        }
        channel.kind = kind;
        0
    }

    pub(crate) fn set_pan(&mut self, id: i32, pan: i32) -> i32 {
        let Some(channel) = self.slots.get_mut(id as usize) else {
            return 1;
        };
        channel.pan = pan.clamp(-10_000, 10_000);
        0
    }

    pub(crate) fn load(&mut self, id: i32, bytes: &[u8], stop: &AtomicBool) -> i32 {
        let Some(channel) = self.slots.get_mut(id as usize) else {
            return 1;
        };
        channel.clip = None;
        channel.playing = false;
        channel.position = 0.0;
        channel.finished = false;
        channel.fade = None;
        match shinki_emu_sdk::decode_audio(bytes, MAX_DECODED_FRAMES, stop) {
            Ok(clip) => {
                channel.clip = Some(clip);
                0
            }
            Err(_) => 2,
        }
    }

    pub(crate) fn unload(&mut self, id: i32) {
        if let Some(channel) = self.slots.get_mut(id as usize) {
            channel.clip = None;
            channel.playing = false;
            channel.finished = false;
            channel.position = 0.0;
            channel.fade = None;
        }
    }

    pub(crate) fn start(&mut self, id: i32, repeat: bool) -> i32 {
        let Some(channel) = self.slots.get_mut(id as usize) else {
            return 1;
        };
        channel.start(repeat);
        0
    }

    pub(crate) fn stop(&mut self, id: i32) -> i32 {
        let Some(channel) = self.slots.get_mut(id as usize) else {
            return 1;
        };
        channel.stop();
        0
    }

    pub(crate) fn start_fade(&mut self, id: i32, repeat: bool, duration: u32) -> i32 {
        if self
            .slots
            .get(id as usize)
            .is_none_or(|channel| channel.clip.is_none())
        {
            return 3;
        }
        self.start(id, repeat);
        if duration != 0 {
            self.slots[id as usize].fade = Some(Fade {
                elapsed: 0,
                duration: u64::from(duration) * 48,
                start: 0.0,
                stop_when_done: false,
            });
        }
        0
    }

    pub(crate) fn fade_channel_out(&mut self, id: i32, duration: u32) -> i32 {
        let Some(channel) = self
            .slots
            .get_mut(id as usize)
            .filter(|channel| channel.clip.is_some())
        else {
            return 2;
        };
        if !channel.playing {
            return -1;
        }
        if duration == 0 {
            return self.stop(id);
        }
        channel.fade = Some(Fade {
            elapsed: 0,
            duration: u64::from(duration) * 48,
            start: channel.fade.as_ref().map_or(1.0, Fade::gain),
            stop_when_done: true,
        });
        0
    }

    pub(crate) fn handle(&self, id: i32) -> i32 {
        if self.valid(id) {
            CHANNEL_HANDLE | id
        } else {
            0
        }
    }

    pub(crate) fn music_handle(&self) -> i32 {
        MUSIC_HANDLE | i32::from(self.music_slot)
    }

    fn channel_for_handle(&self, handle: i32) -> Option<&Channel> {
        if handle == self.music_handle() {
            Some(&self.music)
        } else if handle == (self.music_handle() ^ 1) {
            self.fading_music.as_ref()
        } else {
            self.slots.get((handle ^ CHANNEL_HANDLE) as usize)
        }
    }

    fn channel_for_handle_mut(&mut self, handle: i32) -> Option<&mut Channel> {
        if handle == self.music_handle() {
            Some(&mut self.music)
        } else if handle == (self.music_handle() ^ 1) {
            self.fading_music.as_mut()
        } else {
            self.slots.get_mut((handle ^ CHANNEL_HANDLE) as usize)
        }
    }

    pub(crate) fn handle_playing(&self, handle: i32) -> bool {
        self.channel_for_handle(handle)
            .is_some_and(|channel| channel.playing)
    }

    pub(crate) fn set_handle_gain(&mut self, handle: i32, gain: i32) -> i32 {
        let Some(channel) = self.channel_for_handle_mut(handle) else {
            return -2;
        };
        if gain >= 0 {
            channel.gain = gain.min(256);
        }
        0
    }

    pub(crate) fn control_handle(&mut self, handle: i32, operation: u32) -> i32 {
        let Some(channel) = self.channel_for_handle_mut(handle) else {
            return -1;
        };
        if channel.clip.is_none() {
            return 1;
        }
        match operation {
            601 | 602 => channel.start(operation == 602),
            603 => channel.playing = false,
            604 => channel.playing = true,
            605 => channel.stop(),
            _ => unreachable!("checked Lucifen sound operation"),
        }
        0
    }

    pub(crate) fn time_ms(&self, handle: i32, total: bool) -> i32 {
        self.channel_for_handle(handle)
            .and_then(|channel| {
                let clip = channel.clip.as_ref()?;
                let count = if total {
                    clip.frames.len() as f64
                } else {
                    channel.position
                };
                Some((count * 1000.0 / f64::from(clip.sample_rate)) as i32)
            })
            .unwrap_or_default()
    }

    pub(crate) fn speed(&mut self, id: i32, rate: f64) -> i32 {
        let Some(channel) = self.slots.get_mut(id as usize) else {
            return 1;
        };
        channel.speed = if rate.is_finite() && rate > 0.0 {
            rate.clamp(0.125, 8.0)
        } else {
            1.0
        };
        0
    }

    pub(crate) fn playing(&self, id: i32) -> bool {
        self.slots
            .get(id as usize)
            .is_some_and(|channel| channel.playing)
    }

    pub(crate) fn finished(&self, id: i32) -> bool {
        self.slots
            .get(id as usize)
            .is_some_and(|channel| channel.finished)
    }

    pub(crate) fn music_is_playing(&self, name: &str, intro: Option<&str>) -> bool {
        self.music.playing
            && self
                .music_identity
                .as_ref()
                .is_some_and(|(current, opening)| {
                    current.eq_ignore_ascii_case(name)
                        && intro.is_none_or(|intro| {
                            opening
                                .as_ref()
                                .is_some_and(|opening| opening.eq_ignore_ascii_case(intro))
                        })
                })
    }

    pub(crate) fn play_music(
        &mut self,
        source: MusicSource<'_>,
        repeat: bool,
        fade_ms: u32,
        stop: &AtomicBool,
    ) -> i32 {
        let Some((channel, identity)) = source.open(stop) else {
            return 2;
        };
        self.fading_music = None;
        self.crossfade = false;
        self.music = Channel {
            playing: true,
            repeat,
            fade: (fade_ms != 0).then(|| Fade {
                elapsed: 0,
                duration: u64::from(fade_ms) * 48,
                start: 0.0,
                stop_when_done: false,
            }),
            ..channel
        };
        self.music_identity = Some(identity);
        0
    }

    pub(crate) fn crossfade_music(
        &mut self,
        source: MusicSource<'_>,
        repeat: bool,
        transition: Crossfade,
        stop: &AtomicBool,
    ) -> i32 {
        let Some((channel, identity)) = source.open(stop) else {
            return 2;
        };
        let volume = if transition.volume < 0 {
            self.music.volume
        } else {
            transition.volume.min(100)
        };
        let duration = u64::from(transition.duration) * 48;
        let mut outgoing = std::mem::take(&mut self.music);
        self.music_slot = !self.music_slot;
        let has_outgoing = outgoing.playing && duration != 0;
        let delay = if has_outgoing {
            u64::from(transition.duration.saturating_sub(transition.overlap)) * 48
        } else {
            0
        };
        self.music = Channel {
            playing: true,
            repeat,
            volume,
            delay,
            fade: (duration != 0).then_some(Fade {
                elapsed: 0,
                duration,
                start: 0.0,
                stop_when_done: false,
            }),
            ..channel
        };
        if has_outgoing {
            outgoing.delay = 0;
            outgoing.fade = Some(Fade {
                elapsed: 0,
                duration,
                start: outgoing.fade.as_ref().map_or(1.0, Fade::gain),
                stop_when_done: true,
            });
            self.fading_music = Some(outgoing);
        } else {
            self.fading_music = None;
        }
        self.music_identity = Some(identity);
        self.crossfade = duration != 0;
        0
    }

    pub(crate) fn stop_music(&mut self) {
        self.music = Channel::default();
        self.music_identity = None;
        self.fading_music = None;
        self.crossfade = false;
    }

    pub(crate) fn fade_out_music(&mut self, duration_ms: u32) -> i32 {
        if !self.music.playing {
            self.stop_music();
            return -1;
        }
        if duration_ms == 0 {
            self.stop_music();
            return 0;
        }
        self.music.fade = Some(Fade {
            elapsed: 0,
            duration: u64::from(duration_ms) * 48,
            start: self.music.fade.as_ref().map_or(1.0, Fade::gain),
            stop_when_done: true,
        });
        self.music.delay = 0;
        self.fading_music = None;
        self.crossfade = false;
        0
    }

    pub(crate) fn music_status(&self, fade: bool) -> i32 {
        if fade {
            if self.crossfade {
                return 4;
            }
            self.music
                .fade
                .as_ref()
                .map_or(0, |fade| 2 + i32::from(fade.stop_when_done))
        } else {
            i32::from(self.music.playing)
        }
    }

    fn any_playing(&self) -> bool {
        self.music.playing
            || self
                .fading_music
                .as_ref()
                .is_some_and(|channel| channel.playing)
            || self.slots.iter().any(|channel| channel.playing)
    }

    pub(crate) fn mix(
        &mut self,
        now_ms: u32,
        output: &mut shinki_emu_sdk::NativeAudioOutput,
        stop: &AtomicBool,
    ) -> io::Result<()> {
        let start = self.previous_ms.replace(now_ms).unwrap_or(now_ms);
        let frames = (now_ms.wrapping_sub(start) as usize).min(40) * 48;
        if frames == 0 || !self.any_playing() {
            return Ok(());
        }
        let mut mix = vec![0.0f32; frames * 2];
        for channel in self
            .slots
            .iter_mut()
            .chain(std::iter::once(&mut self.music))
            .chain(self.fading_music.iter_mut())
            .filter(|channel| channel.playing)
        {
            if channel.clip.is_none() {
                continue;
            }
            let volume = channel.volume as f32 * channel.gain as f32 / 25_600.0;
            let left = 10.0_f32.powf(-channel.pan.max(0) as f32 / 2000.0);
            let right = 10.0_f32.powf(channel.pan.min(0) as f32 / 2000.0);
            for frame in mix.chunks_exact_mut(2) {
                if channel.delay != 0 {
                    channel.delay -= 1;
                    continue;
                }
                let clip = channel.clip.as_ref().unwrap();
                let index = channel.position as usize;
                if index >= clip.frames.len() {
                    if let Some(next) = channel.following.take() {
                        let tail = (channel.position - clip.frames.len() as f64)
                            / f64::from(clip.sample_rate);
                        channel.position = (tail * f64::from(next.sample_rate))
                            .min((next.frames.len() - 1) as f64);
                        channel.clip = Some(next);
                    } else if channel.repeat {
                        channel.position %= clip.frames.len() as f64;
                    } else {
                        channel.playing = false;
                        channel.finished = true;
                        channel.fade = None;
                        break;
                    }
                }
                let clip = channel.clip.as_ref().unwrap();
                let index = channel.position as usize;
                let weight = (channel.position - index as f64) as f32;
                let a = clip.frames[index];
                let b = clip
                    .frames
                    .get(index + 1)
                    .copied()
                    .or_else(|| {
                        channel
                            .following
                            .as_ref()
                            .and_then(|next| next.frames.first())
                            .copied()
                    })
                    .unwrap_or(if channel.repeat { clip.frames[0] } else { a });
                let volume = if let Some(fade) = &mut channel.fade {
                    let gain = fade.gain();
                    fade.elapsed += 1;
                    if fade.elapsed == fade.duration {
                        channel.playing = !fade.stop_when_done;
                        channel.finished = fade.stop_when_done;
                        channel.fade = None;
                    }
                    volume * gain
                } else {
                    volume
                };
                frame[0] += (a.left + (b.left - a.left) * weight) * volume * left;
                frame[1] += (a.right + (b.right - a.right) * weight) * volume * right;
                channel.position += f64::from(clip.sample_rate) / OUTPUT_RATE * channel.speed;
                if !channel.playing {
                    break;
                }
            }
        }
        if self
            .fading_music
            .as_ref()
            .is_some_and(|channel| !channel.playing)
        {
            self.fading_music = None;
        }
        self.crossfade &= self.fading_music.is_some() || self.music.fade.is_some();
        for sample in &mut mix {
            *sample = sample.clamp(-1.0, 1.0);
        }
        output
            .write(mix, stop)
            .map_err(|_| io::Error::other("Lucifen audio output failed"))?;
        if !self.any_playing() {
            output
                .flush(stop)
                .map_err(|_| io::Error::other("Lucifen audio output flush failed"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kira::sound::static_sound::StaticSoundData;

    #[test]
    fn music_handle_exposes_playback_position_and_track_duration() {
        let mut channels = Channels::default();
        channels.resize(256);
        channels.music.clip = Some(StaticSoundData {
            sample_rate: 1_000,
            frames: vec![kira::Frame::ZERO; 400].into(),
            settings: Default::default(),
            slice: None,
        });
        channels.music.position = 150.0;
        channels.music.playing = true;

        let handle = channels.music_handle();
        assert_ne!(handle, channels.handle(255));
        assert_eq!(channels.time_ms(handle, false), 150);
        assert_eq!(channels.time_ms(handle, true), 400);
        assert_eq!(channels.control_handle(handle, 603), 0);
        assert!(!channels.handle_playing(handle));
        assert_eq!(channels.time_ms(handle, false), 150);
    }
}
