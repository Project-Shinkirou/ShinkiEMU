use super::{DetectionResult, DetectionTree, Scan};
use std::path::{Path, PathBuf};

const SCRIPT_LIMIT: usize = 256 * 1024;
const INDEX_LIMIT: usize = 1024 * 1024;
const SECURE_DAT_MASK: [u8; 5] = [0x79, 0x57, 0x0d, 0x80, 0x04];

#[derive(Clone, Copy)]
enum Script {
    Text,
    Dat,
    SecureDat,
}

#[derive(Default)]
struct Labels(u8);

impl Labels {
    fn read(&mut self, bytes: &[u8]) -> bool {
        for line in bytes.split(|&byte| matches!(byte, b'\r' | b'\n')) {
            let word = line
                .trim_ascii_start()
                .split(|&byte| byte.is_ascii_whitespace() || matches!(byte, b':' | b';'))
                .next()
                .unwrap_or_default();
            match self.0 {
                0 if word.eq_ignore_ascii_case(b"*define") => self.0 = 1,
                1 if word.eq_ignore_ascii_case(b"game") => self.0 = 2,
                2 if word.eq_ignore_ascii_case(b"*start") => return true,
                _ => {}
            }
        }
        false
    }
}

impl<T: DetectionTree> Scan<'_, T> {
    pub(super) fn ons(&mut self, root: &Path, files: &[PathBuf]) -> DetectionResult<()> {
        let Some((path, Some(mode))) = [
            ("0.txt", Some(Script::Text)),
            ("00.txt", Some(Script::Text)),
            ("nscr_sec.dat", Some(Script::SecureDat)),
            ("nscript.___", None), // The native loader needs a separate key table.
            ("nscript.dat", Some(Script::Dat)),
        ]
        .into_iter()
        .find_map(|(name, mode)| named(files, name).map(|path| (path, mode))) else {
            return Ok(());
        };

        let mut labels = Labels::default();
        let mut remaining = SCRIPT_LIMIT;
        let Some(mut recognized) =
            self.ons_script(root, path, mode, &mut labels, &mut remaining)?
        else {
            return Ok(());
        };
        if !recognized && remaining != 0 && matches!(mode, Script::Text) {
            for (path, _) in numbered_parts(files).into_iter().flatten() {
                let Some(found) = self.ons_script(root, path, mode, &mut labels, &mut remaining)?
                else {
                    return Ok(());
                };
                recognized = found;
                if recognized || remaining == 0 {
                    break;
                }
            }
        }
        if !recognized {
            return Ok(());
        }

        for (name, nsa, format) in [("arc.nsa", true, "nsa"), ("arc.sar", false, "sar")] {
            if let Some(path) = named(files, name)
                && self.ons_archive(root, path, nsa)?
            {
                return self.matched("onscripter", format);
            }
        }
        let format = match mode {
            Script::Text => "onscript",
            Script::Dat => "nscript-dat",
            Script::SecureDat => "nscr-sec",
        };
        self.matched("onscripter", format)
    }

    fn ons_script(
        &mut self,
        root: &Path,
        path: &Path,
        mode: Script,
        labels: &mut Labels,
        remaining: &mut usize,
    ) -> DetectionResult<Option<bool>> {
        let Some(mut source) = self.open(&root.join(path), 0)? else {
            return Ok(None);
        };
        let size = source.length.min(*remaining as u64) as usize;
        *remaining -= size;
        let mut data = self.bytes(&mut source, 0, size)?;
        match mode {
            Script::Text => {}
            Script::Dat => data.iter_mut().for_each(|byte| *byte ^= 0x84),
            Script::SecureDat => data
                .iter_mut()
                .zip(SECURE_DAT_MASK.into_iter().cycle())
                .for_each(|(byte, mask)| *byte ^= mask),
        }
        if data
            .iter()
            .any(|&byte| byte < b' ' && !matches!(byte, b'\r' | b'\n' | b'\t'))
        {
            return Ok(None);
        }
        if (size as u64) < source.length {
            let complete_end = data
                .iter()
                .rposition(|&byte| matches!(byte, b'\r' | b'\n'))
                .map_or(0, |end| end + 1);
            data.truncate(complete_end);
        }
        self.checkpoint()?;
        Ok(Some(labels.read(&data)))
    }

    fn ons_archive(&mut self, root: &Path, path: &Path, nsa: bool) -> DetectionResult<bool> {
        let Some(mut source) = self.open(&root.join(path), 0)? else {
            return Ok(false);
        };
        if source.length < 7 {
            return Ok(false);
        }
        let mut header = [0; 6];
        self.read_at(&mut source, 0, &mut header)?;
        let count = usize::from(u16::from_be_bytes([header[0], header[1]]));
        let base = be32(&header[2..]) as usize;
        let fields = if nsa { 13 } else { 8 };
        if count == 0
            || base < 6 + count * (2 + fields)
            || base > INDEX_LIMIT
            || base as u64 >= source.length
        {
            return Ok(false);
        }
        let index = self.bytes(&mut source, 6, base - 6)?;
        let mut cursor = index.as_slice();
        let mut any_data = false;
        for _ in 0..count {
            self.checkpoint()?;
            let Some(name_len) = cursor.iter().take(256).position(|&byte| byte == 0) else {
                return Ok(false);
            };
            if name_len == 0 || cursor[..name_len].iter().any(|&byte| byte < b' ') {
                return Ok(false);
            }
            cursor = &cursor[name_len + 1..];
            let Some(record) = cursor.get(..fields) else {
                return Ok(false);
            };
            cursor = &cursor[fields..];
            let fields = if nsa {
                if !matches!(record[0], 0 | 1 | 2 | 4) {
                    return Ok(false);
                }
                &record[1..]
            } else {
                record
            };
            let offset = be32(&fields[..4]);
            let size = be32(&fields[4..8]);
            if base as u64 + offset + size > source.length {
                return Ok(false);
            }
            any_data |= size != 0;
        }
        Ok(any_data)
    }
}

fn be32(bytes: &[u8]) -> u64 {
    u64::from(u32::from_be_bytes(
        bytes.try_into().expect("four-byte field"),
    ))
}

fn numbered_parts(files: &[PathBuf]) -> [Option<(&Path, bool)>; 99] {
    let mut parts = [None; 99];
    for path in files {
        let Some(name) = path.file_name().map(|name| name.as_encoded_bytes()) else {
            continue;
        };
        let Some(split) = name.len().checked_sub(4) else {
            continue;
        };
        let (stem, extension) = name.split_at(split);
        if !extension.eq_ignore_ascii_case(b".txt") {
            continue;
        }
        let (index, padded) = match *stem {
            [b'0', digit @ b'1'..=b'9'] => (digit - b'0', true),
            [digit @ b'1'..=b'9'] => (digit - b'0', false),
            [tens @ b'1'..=b'9', units @ b'0'..=b'9'] => (10 * (tens - b'0') + units - b'0', false),
            _ => continue,
        };
        let slot = &mut parts[usize::from(index - 1)];
        if slot.is_none_or(|(_, previous_padded)| previous_padded && !padded) {
            *slot = Some((path.as_path(), padded));
        }
    }
    parts
}

fn named<'a>(files: &'a [PathBuf], name: &str) -> Option<&'a Path> {
    files
        .iter()
        .find(|path| {
            path.file_name()
                .is_some_and(|entry| entry.eq_ignore_ascii_case(name))
        })
        .map(PathBuf::as_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NativeCancellation, detection::detect_game_directory};
    use std::fs;

    const SCRIPT: &[u8] = b"*define\ngame\n*start\nend\n";

    fn ons_format(root: &Path) -> Option<String> {
        detect_game_directory(root, &NativeCancellation::new(1))
            .unwrap()
            .into_iter()
            .find(|detection| detection.family_id == "onscripter")
            .map(|detection| detection.format)
    }

    #[test]
    fn ons_script_precedence_and_split_detection() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::write(root.join("00.TXT"), b";mode800\r\n*define\r\ngame\r\n").unwrap();
        fs::write(root.join("01.TXT"), b"*start\n\x93\xfa\x96\x7b@\nend\n").unwrap();
        assert_eq!(ons_format(root).as_deref(), Some("onscript"));

        let dat = SCRIPT.iter().map(|byte| byte ^ 0x84).collect::<Vec<_>>();
        fs::write(root.join("NSCRIPT.DAT"), dat).unwrap();
        fs::write(root.join("0.txt"), b"ordinary numbered text\n*start\n").unwrap();
        assert_eq!(ons_format(root), None);
        for name in ["0.txt", "00.TXT", "01.TXT"] {
            fs::remove_file(root.join(name)).unwrap();
        }
        assert_eq!(ons_format(root).as_deref(), Some("nscript-dat"));
        fs::write(root.join("nscript.___"), []).unwrap();
        assert_eq!(ons_format(root), None);
        let secure = SCRIPT
            .iter()
            .zip(SECURE_DAT_MASK.into_iter().cycle())
            .map(|(byte, mask)| byte ^ mask)
            .collect::<Vec<_>>();
        fs::write(root.join("NSCR_SEC.DAT"), secure).unwrap();
        assert_eq!(ons_format(root).as_deref(), Some("nscr-sec"));
    }

    #[test]
    fn ons_archives_require_script_and_member_within_file() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        for (name, nsa, format) in [("ARC.NSA", true, "nsa"), ("ARC.SAR", false, "sar")] {
            fs::write(root.join("0.txt"), b"A list of numbered notes\n").unwrap();
            let archive = root.join(name);
            let mut bytes = vec![0, 1, 0, 0, 0, if nsa { 25 } else { 20 }];
            bytes.extend_from_slice(b"a.bmp\0");
            if nsa {
                bytes.push(0);
            }
            bytes.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1]);
            if nsa {
                bytes.extend_from_slice(&[0, 0, 0, 1]);
            }
            bytes.push(1);
            fs::write(&archive, &bytes).unwrap();
            assert_eq!(ons_format(root), None);
            fs::write(root.join("0.txt"), SCRIPT).unwrap();
            assert_eq!(ons_format(root).as_deref(), Some(format));
            bytes.pop();
            fs::write(&archive, &bytes).unwrap();
            assert_eq!(ons_format(root).as_deref(), Some("onscript"));
            fs::remove_file(archive).unwrap();
        }
    }
}
