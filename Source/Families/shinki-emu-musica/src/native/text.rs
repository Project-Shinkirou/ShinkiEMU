use crate::scene::error;
use shinki_emu_abi as abi;
use shinki_emu_sdk::runtime::SessionResult;
use std::sync::{atomic::Ordering, Arc};

pub(crate) use shinki_emu_sdk::native::ServiceContext;
#[derive(Clone)]
pub(crate) struct TextBridge(pub Arc<ServiceContext>, pub abi::AsyncTextServiceV1);
// Host callbacks are thread-safe and their storage survives the native worker.
unsafe impl Send for TextBridge {}
unsafe impl Sync for TextBridge {}

pub(crate) struct PendingReplacement {
    id: u64,
    epoch: u64,
    kinds: Vec<u32>,
    bytes: Vec<u8>,
    ranges: Vec<abi::TextRange>,
}
impl TextBridge {
    pub fn invalidate(&self) {
        self.0.invalidate();
        unsafe {
            self.1.invalidate.unwrap()(
                self.1.context,
                self.0.session,
                self.0.epoch.load(Ordering::Acquire),
            )
        };
    }
    pub fn check_cancelled(&self) -> SessionResult<()> {
        if self.0.is_cancelled() {
            Err(error(
                "SHINKI_EMU_MUSICA_CANCELLED",
                "native session was cancelled",
            ))
        } else {
            Ok(())
        }
    }
    pub fn submit(&self, id: u64, source: &[(u32, &str)]) -> SessionResult<PendingReplacement> {
        let total = source.iter().map(|(_, text)| text.len()).sum::<usize>();
        if source.is_empty()
            || source.len() > 128
            || total > 1024 * 1024
            || source
                .iter()
                .any(|(_, text)| text.len() > shinki_emu_sdk::runtime::MAX_TEXT_BYTES)
        {
            return Err(error(
                "SHINKI_EMU_MUSICA_TEXT_BOUND",
                "semantic text request exceeds its bounds",
            ));
        }
        self.check_cancelled()?;
        let epoch = self.0.epoch.load(Ordering::Acquire);
        let items: Vec<_> = source
            .iter()
            .enumerate()
            .map(|(index, (kind, text))| abi::TextItem {
                id: index as u64 + 1,
                kind: *kind,
                reserved: 0,
                text: abi::Bytes::new(text.as_bytes()),
            })
            .collect();
        let request = abi::TextRequest {
            session_epoch: self.0.session,
            context_epoch: epoch,
            request_id: id,
            items: items.as_ptr(),
            item_count: items.len(),
        };
        let mut diagnostic = abi::Error::default();
        let status = unsafe { self.1.submit.unwrap()(self.1.context, &request, &mut diagnostic) };
        self.check_epoch(epoch)?;
        if status != abi::OK {
            return Err(service_error(&diagnostic));
        }
        Ok(PendingReplacement {
            id,
            epoch,
            kinds: source.iter().map(|(kind, _)| *kind).collect(),
            bytes: vec![0; total.max(1024)],
            ranges: vec![
                abi::TextRange {
                    id: 0,
                    offset: 0,
                    len: 0
                };
                source.len()
            ],
        })
    }
    fn check_epoch(&self, epoch: u64) -> SessionResult<()> {
        if self.0.is_cancelled() || self.0.epoch.load(Ordering::Acquire) != epoch {
            Err(error(
                "SHINKI_EMU_MUSICA_TEXT_CANCELLED",
                "late text result rejected",
            ))
        } else {
            Ok(())
        }
    }
    pub fn poll(&self, pending: &mut PendingReplacement) -> SessionResult<Option<Vec<String>>> {
        self.check_epoch(pending.epoch)?;
        let mut resized = false;
        loop {
            let mut result = abi::TextResult {
                bytes: pending.bytes.as_mut_ptr(),
                byte_capacity: pending.bytes.len(),
                byte_count: 0,
                items: pending.ranges.as_mut_ptr(),
                item_capacity: pending.ranges.len(),
                item_count: 0,
            };
            let mut diagnostic = abi::Error::default();
            let status = unsafe {
                self.1.poll.unwrap()(
                    self.1.context,
                    self.0.session,
                    pending.epoch,
                    pending.id,
                    &mut result,
                    &mut diagnostic,
                )
            };
            self.check_epoch(pending.epoch)?;
            if status == abi::PENDING {
                return Ok(None);
            }
            if status == abi::CAPACITY {
                if resized
                    || result.byte_count > 1024 * 1024
                    || result.item_count != pending.kinds.len()
                    || result.byte_count <= pending.bytes.len()
                {
                    return Err(error(
                        "SHINKI_EMU_MUSICA_TEXT_BOUND",
                        "invalid text result capacity",
                    ));
                }
                pending.bytes.resize(result.byte_count, 0);
                resized = true;
                continue;
            }
            if status != abi::OK {
                return Err(service_error(&diagnostic));
            }
            if result.byte_count > pending.bytes.len() || result.item_count != pending.kinds.len() {
                return Err(error(
                    "SHINKI_EMU_MUSICA_TEXT_BOUND",
                    "invalid text result length",
                ));
            }
            let mut transformed = Vec::with_capacity(pending.kinds.len());
            for (index, range) in pending.ranges.iter().enumerate() {
                let end = range
                    .offset
                    .checked_add(range.len)
                    .filter(|end| *end <= result.byte_count)
                    .ok_or_else(|| error("SHINKI_EMU_MUSICA_TEXT_BOUND", "invalid text range"))?;
                let limit = if pending.kinds[index] == abi::TEXT_SPEAKER {
                    shinki_emu_sdk::runtime::MAX_SYMBOL_BYTES
                } else {
                    shinki_emu_sdk::runtime::MAX_TEXT_BYTES
                };
                if range.id != index as u64 + 1 || range.len > limit {
                    return Err(error(
                        "SHINKI_EMU_MUSICA_TEXT_ID",
                        "text result identity or length differs",
                    ));
                }
                let text =
                    std::str::from_utf8(&pending.bytes[range.offset..end]).map_err(|_| {
                        error("SHINKI_EMU_MUSICA_TEXT_UTF8", "text result is not UTF-8")
                    })?;
                if text.contains('\0') {
                    return Err(error(
                        "SHINKI_EMU_MUSICA_TEXT_UTF8",
                        "text result contains NUL",
                    ));
                }
                transformed.push(text.to_owned());
            }
            if unsafe {
                self.1.acknowledge.unwrap()(
                    self.1.context,
                    self.0.session,
                    pending.epoch,
                    pending.id,
                )
            } != abi::OK
            {
                return Err(error(
                    "SHINKI_EMU_MUSICA_TEXT_ACK",
                    "text result acknowledgement failed",
                ));
            }
            self.check_epoch(pending.epoch)?;
            return Ok(Some(transformed));
        }
    }
    pub fn skip(&self, id: u64) {
        unsafe {
            self.1.cancel.unwrap()(
                self.1.context,
                self.0.session,
                self.0.epoch.load(Ordering::Acquire),
                id,
            )
        };
    }
}
fn service_error(diagnostic: &abi::Error) -> shinki_emu_sdk::runtime::SessionError {
    let code = std::str::from_utf8(
        &diagnostic.code[..(diagnostic.code_len as usize).min(diagnostic.code.len())],
    )
    .ok()
    .filter(|code| !code.is_empty())
    .unwrap_or("SHINKI_EMU_MUSICA_TEXT_FAILED");
    let message = std::str::from_utf8(
        &diagnostic.message[..(diagnostic.message_len as usize).min(diagnostic.message.len())],
    )
    .unwrap_or("text operation failed");
    error(code, message)
}
