use std::io;

pub struct Lgm<'a> {
    pub background: Option<&'a [u8]>,
    pub controls: Vec<LayoutControl<'a>>,
}

pub struct LayoutControl<'a> {
    pub image: &'a [u8],
    pub kind: u8,
    pub x: i16,
    pub y: i16,
    pub opacity: u8,
    pub visible: bool,
    pub extent: [i16; 2],
    pub states: u8,
}

impl<'a> Lgm<'a> {
    pub fn parse(data: &'a [u8]) -> io::Result<Self> {
        let mut data = data.strip_prefix(b"LGM\0").ok_or_else(invalid)?;
        let background = read_name(&mut data)?;
        let count = u16::from_le_bytes(take(&mut data, 2)?.try_into().unwrap());
        if usize::from(count) > data.len() / 13 {
            return Err(invalid());
        }
        let mut controls = Vec::with_capacity(count.into());
        for _ in 0..count {
            let image = read_name(&mut data)?.ok_or_else(invalid)?;
            let control = take(&mut data, 12)?;
            let kind = control[0];
            if !(1..=9).contains(&kind) {
                return Err(invalid());
            }
            controls.push(LayoutControl {
                image,
                kind,
                x: i16::from_le_bytes([control[1], control[2]]),
                y: i16::from_le_bytes([control[3], control[4]]),
                opacity: control[5],
                visible: control[6] != 0,
                extent: [
                    i16::from_le_bytes([control[7], control[8]]),
                    i16::from_le_bytes([control[9], control[10]]),
                ],
                states: control[11],
            });
        }
        Ok(Self {
            background,
            controls,
        })
    }
}

fn read_name<'a>(data: &mut &'a [u8]) -> io::Result<Option<&'a [u8]>> {
    let length = take(data, 1)?[0] as usize;
    if length == 0 {
        return Ok(None);
    }
    let bytes = take(data, length)?;
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .ok_or_else(invalid)?;
    if end == 0 {
        return Err(invalid());
    }
    Ok(Some(&bytes[..end]))
}

fn take<'a>(data: &mut &'a [u8], length: usize) -> io::Result<&'a [u8]> {
    let (value, remaining) = data.split_at_checked(length).ok_or_else(invalid)?;
    *data = remaining;
    Ok(value)
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid Lucifen LGM layout")
}
