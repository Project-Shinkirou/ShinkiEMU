use shinki_emu_sdk::CoreError;

const FLOAT_CAPACITY: usize = 0x40000 / 4;
const DECODER_BYTE_CAPACITY: u32 = 0x20000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Boundary {
    Allocation,
    InputCursor,
    Correlation,
    ReturnedSample,
    EmptyLoop,
    OutputCapacity,
}

/// One native notification, before the ordinary device-rate resampler.
#[derive(Debug)]
pub(crate) struct NativeChunk {
    pub(crate) initialized_samples: usize,
    pub(crate) finished: bool,
    pub(crate) indeterminate_tail: bool,
    /// Increment applied to native stream+148, including a short final read.
    pub(crate) native_decoded_bytes: u32,
    pub(crate) decoder_requested_bytes: u32,
    pub(crate) decoder_returned_bytes: u32,
}

/// cmvs32 48EC80/48EE20/48EE90. Unknown native heap slots stay unknown.
///
/// The complete source is mono signed 16-bit PCM at its original sample rate.
/// Native notifications are independent of the host's output callback size.
/// Physical slots retain earlier writes, including stale EOF scratch samples.
pub(crate) struct NativeMono175 {
    rate: f32,
    source_rate: u32,
    device_rate: u32,
    window: usize,
    min_period: usize,
    max_period: usize,
    source_chunk_bytes: u32,
    looping: bool,
    source_cursor: usize,
    source_repeated: bool,
    input: Box<[Option<f32>]>,
    input_read: usize,
    input_write: usize,
    output: Box<[Option<f32>]>,
    output_read: usize,
    output_write: usize,
    scratch: Box<[Option<i16>]>,
    active: bool,
    done: bool,
    blocked: Option<Boundary>,
}

impl NativeMono175 {
    pub(crate) fn new(
        source_rate: u32,
        rate: f32,
        looping: bool,
        device_rate: u32,
        native_chunk_bytes: u32,
    ) -> Result<Self, CoreError> {
        if !matches!(source_rate, 22050 | 44100) || !matches!(device_rate, 22050 | 44100) {
            return Err(CoreError::invalid(
                "SHINKI_EMU_CMVS_NATIVE_AUDIO_STRETCH_FORMAT",
                "native mono stretch requires a 22050 or 44100 Hz source and device",
            ));
        }
        if !((rate as f64) >= 1.1 && (rate as f64) <= 3.0) {
            return Err(CoreError::invalid(
                "SHINKI_EMU_CMVS_NATIVE_AUDIO_STRETCH_RATE",
                "native mono stretch is active only for rates from 1.1 through 3",
            ));
        }
        let full_rate_chunk = if device_rate == 22050 {
            native_chunk_bytes.checked_mul(2)
        } else {
            Some(native_chunk_bytes)
        };
        if !matches!(full_rate_chunk, Some(6144 | 8192 | 12288 | 16384)) {
            return Err(CoreError::invalid(
                "SHINKI_EMU_CMVS_NATIVE_AUDIO_STRETCH_CONFIG",
                "native mono stretch requires a native DirectSound notification size",
            ));
        }
        let mut source_chunk_bytes = native_chunk_bytes / 2;
        if device_rate == 22050 && source_rate == 44100 {
            source_chunk_bytes *= 2;
        } else if device_rate == 44100 && source_rate == 22050 {
            source_chunk_bytes /= 2;
        }
        let sr = (source_rate as f32) as f64;
        Ok(Self {
            rate,
            source_rate,
            device_rate,
            window: (sr * 0.005).trunc() as usize,
            min_period: (sr * 0.0025).trunc() as usize,
            max_period: (sr * 0.01).trunc() as usize,
            source_chunk_bytes,
            looping,
            source_cursor: 0,
            source_repeated: false,
            input: vec![None; FLOAT_CAPACITY].into_boxed_slice(),
            input_read: 0,
            input_write: 0,
            output: vec![None; FLOAT_CAPACITY].into_boxed_slice(),
            output_read: 0,
            output_write: 0,
            scratch: vec![None; (source_chunk_bytes / 2) as usize].into_boxed_slice(),
            active: true,
            done: false,
            blocked: None,
        })
    }

    pub(crate) fn source_chunk_samples(&self) -> usize {
        (self.source_chunk_bytes / 2) as usize
    }

    pub(crate) fn source_rate(&self) -> u32 {
        self.source_rate
    }

    pub(crate) fn device_rate(&self) -> u32 {
        self.device_rate
    }

    pub(crate) fn decoder_position_samples(&self) -> usize {
        self.source_cursor
    }

    pub(crate) fn decoder_repeated(&self) -> bool {
        self.source_repeated
    }

    pub(crate) fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
    }

    /// 48CA10 rewinds the decoder, not the still-owned stretch queues.
    pub(crate) fn restart(&mut self, looping: bool) {
        self.source_cursor = 0;
        self.source_repeated = false;
        self.looping = looping;
    }

    fn sample(&self, relative: usize) -> Result<f32, Boundary> {
        let index = self
            .input_read
            .checked_add(relative)
            .ok_or(Boundary::Allocation)?;
        self.input
            .get(index)
            .ok_or(Boundary::Allocation)?
            .ok_or(Boundary::Correlation)
    }

    fn to_native_i16(value: f32) -> i16 {
        let scaled = value * 32767.0_f32;
        // CVTTSS2SI returns the indefinite integer for NaN/out-of-range input.
        let integer =
            if !scaled.is_finite() || scaled >= 2147483648.0_f32 || scaled < -2147483648.0_f32 {
                i32::MIN
            } else {
                scaled.trunc() as i32
            };
        integer.clamp(-32768, 32767) as i16
    }

    fn drain(&mut self, destination: usize, count: usize) {
        for i in 0..count {
            self.scratch[destination + i] =
                self.output[self.output_read + i].map(Self::to_native_i16);
        }
        self.output_read += count;
    }

    fn generate_block(&mut self) -> Result<(), Boundary> {
        let mut period = self.min_period;
        let mut best = 0.0_f32;
        for candidate in self.min_period..=self.max_period {
            let mut correlation = 0.0_f32;
            for i in 0..self.window {
                let product = self.sample(i)? * self.sample(i + candidate)?;
                // Ordered SSE f32 additions: neither FMA nor a parallel reduction.
                correlation += product;
            }
            if correlation > best {
                best = correlation;
                period = candidate;
            }
        }
        if self.output_write + period > FLOAT_CAPACITY {
            return Err(Boundary::Allocation);
        }
        for i in 0..period {
            let a = *self
                .input
                .get(self.input_read + i)
                .ok_or(Boundary::Allocation)?;
            let b = *self
                .input
                .get(self.input_read + period + i)
                .ok_or(Boundary::Allocation)?;
            self.output[self.output_write + i] = a.zip(b).map(|(a, b)| {
                let first = ((period - i) as f32) * a;
                let second = (i as f32) * b;
                (first + second) / period as f32
            });
        }
        let count = ((period as f64) / ((self.rate as f64) - 1.0) + 0.5).trunc() as usize;
        if self.output_write + count > FLOAT_CAPACITY {
            return Err(Boundary::Allocation);
        }
        for i in period..count {
            if !self.active && period + i >= self.input_write - self.input_read {
                self.done = true;
                break;
            }
            self.output[self.output_write + i] = *self
                .input
                .get(self.input_read + period + i)
                .ok_or(Boundary::Allocation)?;
        }
        // For rates >2, count<period: native excludes the excess blend tail.
        // Even an early EOF copy break still advances by the full count.
        self.input_read += period + count;
        self.output_write += count;
        Ok(())
    }

    fn append_decode(
        &mut self,
        pcm: &[i16],
        repeat: Option<&[i16]>,
        returned_samples: usize,
        end_of_input: bool,
    ) -> Result<(), Boundary> {
        if self.input_read > self.input_write {
            return Err(Boundary::InputCursor);
        }
        let remaining = self.input_write - self.input_read;
        if remaining != 0 {
            self.input.copy_within(self.input_read..self.input_write, 0);
        }
        self.input_read = 0;
        self.input_write = remaining;
        if remaining + returned_samples > FLOAT_CAPACITY {
            return Err(Boundary::Allocation);
        }
        for i in 0..returned_samples {
            let source = if self.source_repeated {
                repeat.unwrap_or(pcm)
            } else {
                pcm
            };
            let sample = *source
                .get(self.source_cursor)
                .ok_or(Boundary::ReturnedSample)?;
            self.input[self.input_write + i] = Some((sample as f32) * (1.0_f32 / 32768.0_f32));
            self.source_cursor += 1;
            if self.looping && self.source_cursor == source.len() {
                self.source_cursor = 0;
                self.source_repeated = true;
            }
        }
        self.input_write += returned_samples;
        if end_of_input {
            self.active = false;
            let physical_end = self.input_write + 4 * self.max_period;
            if physical_end > FLOAT_CAPACITY {
                return Err(Boundary::Allocation);
            }
            // 48F1A4: memset16*Pmax bytes, but logical float count advances2Pmax.
            self.input[self.input_write..physical_end].fill(Some(0.0));
            self.input_write += 2 * self.max_period;
        }
        Ok(())
    }

    /// Writes only the proven prefix of one native notification into `output`.
    /// The caller must neither play nor fabricate samples beyond that prefix.
    /// Source PCM and this state must persist across host output callbacks.
    pub(crate) fn next_chunk(
        &mut self,
        pcm: &[i16],
        repeat: Option<&[i16]>,
        output: &mut [f32],
    ) -> NativeChunk {
        let count = self.source_chunk_samples();
        let mut result = NativeChunk {
            initialized_samples: 0,
            finished: self.done,
            indeterminate_tail: self.blocked.is_some(),
            native_decoded_bytes: self.source_chunk_bytes,
            decoder_requested_bytes: 0,
            decoder_returned_bytes: 0,
        };
        if self.done || self.blocked.is_some() {
            result.native_decoded_bytes = 0;
            return result;
        }
        if output.len() < count {
            self.blocked = Some(Boundary::OutputCapacity);
            result.indeterminate_tail = true;
            return result;
        }
        let queued = self.output_write - self.output_read;
        if (queued as u32).wrapping_mul(2) < self.source_chunk_bytes
            && self.input_write.wrapping_sub(self.input_read) < 0x4000
            && self.active
        {
            let missing = self
                .source_chunk_bytes
                .wrapping_sub((queued as u32).wrapping_mul(2));
            // Native promotes rate to f64: 2.1f32 itself is BELOW this threshold.
            result.decoder_requested_bytes =
                missing.wrapping_mul(if (self.rate as f64) >= 2.1 { 8 } else { 5 });
        }
        if result.decoder_requested_bytes > DECODER_BYTE_CAPACITY {
            self.blocked = Some(Boundary::Allocation);
            result.indeterminate_tail = true;
            return result;
        }
        let source = if self.source_repeated {
            repeat.unwrap_or(pcm)
        } else {
            pcm
        };
        if self.looping && source.is_empty() && result.decoder_requested_bytes != 0 {
            self.blocked = Some(Boundary::EmptyLoop);
            result.indeterminate_tail = true;
            return result;
        }
        let requested_samples = (result.decoder_requested_bytes / 2) as usize;
        let returned_samples = if self.looping {
            requested_samples
        } else {
            requested_samples.min(source.len().saturating_sub(self.source_cursor))
        };
        result.decoder_returned_bytes = (returned_samples * 2) as u32;
        let eof = self.active && result.decoder_returned_bytes < result.decoder_requested_bytes;
        if eof {
            self.active = false;
        }
        let initial = queued.min(count);
        let mut proven_samples = initial;
        self.drain(0, initial);
        let mut boundary = None;
        if queued < count {
            self.output_read = 0;
            self.output_write = 0;
            if let Err(reason) = self.append_decode(pcm, repeat, returned_samples, eof) {
                boundary = Some(reason);
            } else {
                while self.output_write - self.output_read < count - initial {
                    if !self.active && self.input_read > self.input_write {
                        self.done = true;
                        break;
                    }
                    if !self.active && 2 * self.max_period > self.input_write - self.input_read {
                        self.done = true;
                        break;
                    }
                    if let Err(reason) = self.generate_block() {
                        boundary = Some(reason);
                        break;
                    }
                }
                let available = self.output_write - self.output_read;
                let drained = available.min(count - initial);
                self.drain(initial, drained);
                proven_samples += drained;
                if available < count - initial {
                    self.output_read = 0;
                    self.output_write = 0;
                }
            }
        }
        let limit = if boundary.is_some() {
            proven_samples
        } else {
            count
        };
        for (target, &sample) in output.iter_mut().zip(&self.scratch).take(limit) {
            match sample {
                Some(sample) => {
                    *target = (sample as f32) * (1.0_f32 / 32768.0_f32);
                    result.initialized_samples += 1;
                }
                None => {
                    boundary = Some(Boundary::ReturnedSample);
                    break;
                }
            }
        }
        self.blocked = boundary;
        result.indeterminate_tail = boundary.is_some();
        result.finished = self.done;
        result
    }
}

#[cfg(test)]
#[path = "native_stretch/tests.rs"]
mod tests;
