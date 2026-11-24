#[cfg(feature = "video-ffmpeg")]
use crate::movie::MovieAudio;
use crate::{
    Error, Result,
    archive::Archive,
    binary::{dword, range},
    game::{Game, read_bounded},
};
use kira::{
    AudioManager, AudioManagerSettings, Decibels, Panning, Tween,
    backend::{Backend, Renderer},
    sound::{
        PlaybackState,
        static_sound::{StaticSoundData, StaticSoundHandle},
    },
};
#[cfg(target_env = "ohos")]
use shinki_emu_sdk::runtime::{AudioSink, OhosAudioOutput, PcmFormat, PcmFormatSpec};
#[cfg(feature = "video-ffmpeg")]
use std::sync::Mutex;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc::{Receiver, SyncSender, TrySendError, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const MAX_ASSET_BYTES: usize = 64 * 1024 * 1024;
const MAX_DECODED_FRAMES: usize = 24 * 1024 * 1024;

enum Command {
    Play(u8, StaticSoundData, Arc<SoundProgress>),
    Stop(u8),
    Fade(u8, std::time::Duration),
    Pause(u8, bool),
    Seek(u8, u8),
    Volume(u8, f32),
    Pan(u8, f32),
}

struct SoundProgress {
    playing: AtomicBool,
    percent: AtomicU8,
}

pub(crate) struct Audio {
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    commands: SyncSender<Command>,
    active: BTreeMap<u8, Arc<SoundProgress>>,
    voice_archive: Option<Archive>,
    worker: Option<JoinHandle<Result<()>>>,
    #[cfg(target_env = "ohos")]
    sink: Arc<OhosAudioOutput>,
    #[cfg(feature = "video-ffmpeg")]
    movie: Arc<Mutex<Option<Arc<MovieAudio>>>>,
}

impl Audio {
    pub(crate) fn open(#[cfg(not(target_env = "ohos"))] null_audio: bool) -> Result<Self> {
        #[cfg(target_env = "ohos")]
        let sink = Arc::new(OhosAudioOutput::default());
        let stop = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));
        #[cfg(feature = "video-ffmpeg")]
        let movie = Arc::new(Mutex::new(None));
        let (commands, receive) = sync_channel(32);
        let (ready, wait) = sync_channel(1);
        let worker = {
            #[cfg(target_env = "ohos")]
            let sink = sink.clone();
            let stop = stop.clone();
            let paused = paused.clone();
            #[cfg(feature = "video-ffmpeg")]
            let movie = movie.clone();
            thread::Builder::new()
                .name("adv32-audio".into())
                .spawn(move || {
                    run(
                        #[cfg(target_env = "ohos")]
                        sink,
                        #[cfg(not(target_env = "ohos"))]
                        null_audio,
                        &stop,
                        &paused,
                        &receive,
                        &ready,
                        #[cfg(feature = "video-ffmpeg")]
                        &movie,
                    )
                })?
        };
        if wait.recv().is_err() {
            worker
                .join()
                .map_err(|_| Error::Audio("device worker panicked".into()))??;
            return Err(Error::Audio("device worker exited during startup".into()));
        }
        Ok(Self {
            stop,
            paused,
            commands,
            active: BTreeMap::new(),
            voice_archive: None,
            worker: Some(worker),
            #[cfg(target_env = "ohos")]
            sink,
            #[cfg(feature = "video-ffmpeg")]
            movie,
        })
    }

    pub(crate) fn load_and_play(
        &mut self,
        game: &Game,
        name: &str,
        channel: u8,
        repeat: bool,
        gain: f32,
        pan: f32,
    ) -> Result<()> {
        let bytes = read_audio(game, name, &mut self.voice_archive)?;
        let data = decode(&bytes, &self.stop)?
            .volume(decibels(gain))
            .panning(Panning(pan));
        let data = if repeat { data.loop_region(..) } else { data };
        let active = Arc::new(SoundProgress {
            playing: AtomicBool::new(true),
            percent: AtomicU8::new(0),
        });
        self.send(Command::Play(channel, data, active.clone()))?;
        self.active.insert(channel, active);
        Ok(())
    }

    pub(crate) fn stop_channel(&mut self, channel: u8) -> Result<()> {
        if self.active.contains_key(&channel) {
            self.send(Command::Stop(channel))?;
            self.active.remove(&channel);
        }
        Ok(())
    }

    pub(crate) fn playing(&self, channel: u8) -> bool {
        self.active
            .get(&channel)
            .is_some_and(|active| active.playing.load(Ordering::Acquire))
    }

    pub(crate) fn music_position(&self) -> Option<u8> {
        self.active.get(&0).and_then(|track| {
            track
                .playing
                .load(Ordering::Acquire)
                .then(|| track.percent.load(Ordering::Relaxed))
        })
    }

    pub(crate) fn fade_channel(
        &mut self,
        channel: u8,
        duration: std::time::Duration,
    ) -> Result<()> {
        self.send(Command::Fade(channel, duration))
    }

    pub(crate) fn pause_channel(&mut self, channel: u8, paused: bool) -> Result<()> {
        self.send(Command::Pause(channel, paused))
    }

    pub(crate) fn seek_percent(&mut self, channel: u8, percent: u8) -> Result<()> {
        self.send(Command::Seek(channel, percent))
    }

    pub(crate) fn set_volume(&mut self, channel: u8, gain: f32) -> Result<()> {
        self.send(Command::Volume(channel, gain))
    }

    pub(crate) fn set_pan(&mut self, channel: u8, pan: f32) -> Result<()> {
        self.send(Command::Pan(channel, pan))
    }

    #[cfg(feature = "video-ffmpeg")]
    pub(crate) fn set_movie_audio(&self, track: Option<Arc<MovieAudio>>) -> Result<()> {
        *self.movie.lock().map_err(audio_error)? = track;
        Ok(())
    }

    pub(crate) fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Release);
    }

    pub(crate) fn check(&mut self) -> Result<()> {
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
        {
            self.join()?;
        }
        Ok(())
    }

    fn send(&mut self, mut command: Command) -> Result<()> {
        self.check()?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match self.commands.try_send(command) {
                Ok(()) => return Ok(()),
                Err(TrySendError::Full(pending)) if Instant::now() < deadline => {
                    command = pending;
                    thread::sleep(Duration::from_millis(1));
                }
                Err(TrySendError::Full(_)) => {
                    return Err(Error::Audio("device command queue stalled".into()));
                }
                Err(TrySendError::Disconnected(_)) => {
                    self.check()?;
                    return Err(Error::Audio("device command queue disconnected".into()));
                }
            }
        }
    }

    pub(crate) fn join(&mut self) -> Result<()> {
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        self.stop.store(true, Ordering::Release);
        #[cfg(target_env = "ohos")]
        let cancel = self.sink.cancel();
        let worker = worker.join();
        #[cfg(target_env = "ohos")]
        cancel.map_err(audio_error)?;
        worker.map_err(|_| Error::Audio("device worker panicked".into()))?
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        let _ = self.join();
    }
}

fn read_audio(game: &Game, name: &str, voice_archive: &mut Option<Archive>) -> Result<Vec<u8>> {
    let (kind, member) = name
        .split_once(['/', '\\'])
        .ok_or(Error::Invalid("ADV32 audio name"))?;
    if kind.eq_ignore_ascii_case("voice")
        && let Ok(path) = game.asset("voice.dat")
    {
        if voice_archive.is_none() {
            *voice_archive = Some(Archive::open(path)?);
        }
        let archive = voice_archive.as_ref().expect("voice archive was opened");
        let member = member.replace('/', "\\");
        let member = if member
            .rsplit('\\')
            .next()
            .is_some_and(|leaf| leaf.contains('.'))
        {
            member
        } else {
            format!("{member}.pcm")
        };
        if let Some(span) = archive.find_name(&member) {
            return archive.read(span, MAX_ASSET_BYTES);
        }
    }
    let path = game.resource(kind, member, "pcm")?;
    read_bounded(path, MAX_ASSET_BYTES)
}

fn decode(data: &[u8], stop: &AtomicBool) -> Result<StaticSoundData> {
    if !data.starts_with(b"XPCM") {
        return shinki_emu_sdk::decode_audio(data, MAX_DECODED_FRAMES, stop).map_err(audio_error);
    }
    let control = dword(data, 8)?;
    let mode = control & 0xff;
    if mode == 5 {
        let raw = range(data, 16, dword(data, 12)? as usize)?;
        if !raw.starts_with(b"OggS") {
            return Err(Error::Invalid("XPCM Vorbis stream"));
        }
        return shinki_emu_sdk::decode_audio(raw, MAX_DECODED_FRAMES, stop).map_err(audio_error);
    }
    let pcm_header = range(data, 12, 16)?;
    let size = dword(data, 4)?;
    let reconstructed;
    let raw = match mode {
        0 => range(data, 28, size as usize)?,
        1 | 3 => {
            let packed = range(data, 32, dword(data, 28)? as usize)?;
            reconstructed = crate::xpcm::decode_transform(
                packed,
                mode,
                ((control >> 8) & 0xff) as usize,
                size as usize,
                stop,
            )?;
            &reconstructed
        }
        _ => return Err(Error::Unsupported("XPCM compression mode")),
    };
    let padded_size = size
        .checked_add(size & 1)
        .ok_or(Error::Invalid("XPCM length"))?;
    let wave_size = padded_size
        .checked_add(36)
        .ok_or(Error::Invalid("XPCM length"))?;
    let mut wave = Vec::with_capacity(44 + padded_size as usize);
    wave.extend_from_slice(b"RIFF");
    wave.extend(wave_size.to_le_bytes());
    wave.extend_from_slice(b"WAVEfmt \x10\0\0\0");
    wave.extend_from_slice(pcm_header);
    wave.extend_from_slice(b"data");
    wave.extend(size.to_le_bytes());
    wave.extend_from_slice(raw);
    if size & 1 != 0 {
        wave.push(0);
    }
    shinki_emu_sdk::decode_audio(&wave, MAX_DECODED_FRAMES, stop).map_err(audio_error)
}

struct MixerBackend(Option<Renderer>);

struct MixedSound {
    handle: StaticSoundHandle,
    active: Arc<SoundProgress>,
    duration: f64,
}

impl Backend for MixerBackend {
    type Settings = ();
    type Error = std::convert::Infallible;

    fn setup(_: (), _: usize) -> std::result::Result<(Self, u32), Self::Error> {
        Ok((Self(None), RATE))
    }

    fn start(&mut self, renderer: Renderer) -> std::result::Result<(), Self::Error> {
        self.0 = Some(renderer);
        Ok(())
    }
}

fn run(
    #[cfg(target_env = "ohos")] sink: Arc<OhosAudioOutput>,
    #[cfg(not(target_env = "ohos"))] null_audio: bool,
    stop: &Arc<AtomicBool>,
    paused: &AtomicBool,
    commands: &Receiver<Command>,
    ready: &SyncSender<()>,
    #[cfg(feature = "video-ffmpeg")] movie: &Mutex<Option<Arc<MovieAudio>>>,
) -> Result<()> {
    #[cfg(target_env = "ohos")]
    sink.configure(PcmFormatSpec {
        sample_rate: RATE,
        channels: CHANNELS,
        format: PcmFormat::F32,
    })
    .map_err(audio_error)?;
    #[cfg(not(target_env = "ohos"))]
    let mut device = if null_audio {
        shinki_emu_sdk::NativeAudioOutput::open_null()
    } else {
        shinki_emu_sdk::NativeAudioOutput::open(stop.clone())
    }
    .map_err(audio_error)?;

    let mut mixer =
        AudioManager::<MixerBackend>::new(AudioManagerSettings::default()).map_err(audio_error)?;
    let mut sounds = BTreeMap::<u8, MixedSound>::new();
    if ready.send(()).is_err() {
        return Ok(());
    }
    let mut samples = vec![0.0; 512 * usize::from(CHANNELS)];
    while !stop.load(Ordering::Acquire) {
        for command in commands.try_iter().take(32) {
            let (channel, replacement) = match command {
                Command::Play(channel, sound, active) => (channel, Some((sound, active))),
                Command::Stop(channel) => (channel, None),
                Command::Fade(channel, duration) => {
                    if let Some(sound) = sounds.get_mut(&channel) {
                        sound.handle.stop(Tween {
                            duration,
                            ..Tween::default()
                        });
                    }
                    continue;
                }
                Command::Pause(channel, paused) => {
                    if let Some(sound) = sounds.get_mut(&channel) {
                        if paused {
                            sound.handle.pause(Tween::default());
                        } else {
                            sound.handle.resume(Tween::default());
                        }
                    }
                    continue;
                }
                Command::Seek(channel, percent) => {
                    if let Some(sound) = sounds.get_mut(&channel) {
                        sound
                            .handle
                            .seek_to(sound.duration * f64::from(percent) / 100.0);
                        sound.handle.resume(Tween::default());
                    }
                    continue;
                }
                Command::Volume(channel, gain) => {
                    if let Some(sound) = sounds.get_mut(&channel) {
                        sound.handle.set_volume(decibels(gain), Tween::default());
                    }
                    continue;
                }
                Command::Pan(channel, pan) => {
                    if let Some(sound) = sounds.get_mut(&channel) {
                        sound.handle.set_panning(Panning(pan), Tween::default());
                    }
                    continue;
                }
            };
            if let Some(mut current) = sounds.remove(&channel) {
                current.handle.stop(Tween::default());
                current.active.playing.store(false, Ordering::Release);
            }
            if let Some((sound, active)) = replacement {
                let duration = sound.duration().as_secs_f64();
                let handle = mixer.play(sound).map_err(audio_error)?;
                sounds.insert(
                    channel,
                    MixedSound {
                        handle,
                        active,
                        duration,
                    },
                );
            }
        }
        samples.fill(0.0);
        let active = !paused.load(Ordering::Acquire);
        if active {
            let renderer = mixer
                .backend_mut()
                .0
                .as_mut()
                .ok_or_else(|| Error::Audio("mixer renderer unavailable".into()))?;
            renderer.on_start_processing();
            renderer.process(&mut samples, CHANNELS);
            sounds.retain(|channel, sound| {
                let state = sound.handle.state();
                if *channel == 0 && sound.duration > 0.0 {
                    let percent = 100.0 * sound.handle.position() / sound.duration;
                    sound
                        .active
                        .percent
                        .store(percent.clamp(0.0, 100.0) as u8, Ordering::Relaxed);
                }
                sound.active.playing.store(
                    !matches!(state, PlaybackState::Paused | PlaybackState::Stopped),
                    Ordering::Release,
                );
                state != PlaybackState::Stopped
            });
        }
        #[cfg(feature = "video-ffmpeg")]
        let movie_position = if active {
            let track = movie.lock().map_err(audio_error)?.clone();
            track
                .map(|track| track.mix(&mut samples).map(|position| (track, position)))
                .transpose()?
        } else {
            None
        };
        #[cfg(target_env = "ohos")]
        let buffer = sink.write(samples).map_err(audio_error)?;
        #[cfg(not(target_env = "ohos"))]
        let buffer = device.write(samples, stop).map_err(audio_error)?;
        let Some(buffer) = buffer else {
            break;
        };
        #[cfg(feature = "video-ffmpeg")]
        if let Some((track, position)) = movie_position {
            track.accepted(position);
        }
        samples = buffer;
    }
    Ok(())
}

fn audio_error(error: impl std::fmt::Display) -> Error {
    Error::Audio(error.to_string())
}

fn decibels(gain: f32) -> Decibels {
    if gain > 0.0 {
        Decibels(20.0 * gain.log10())
    } else {
        Decibels::SILENCE
    }
}
