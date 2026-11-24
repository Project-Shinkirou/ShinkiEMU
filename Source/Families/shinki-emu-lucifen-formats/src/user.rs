use crate::sob::word;
use std::io;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Variable {
    pub storage: u8,
    pub persistence: u8,
    pub offset: u32,
    pub bytes: u32,
}

pub struct UserVariables {
    pub defaults: Vec<u8>,
    pub variables: Vec<Variable>,
    pub pointers: Vec<(u32, u32)>,
}

impl UserVariables {
    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        if !bytes.starts_with(b"SOB1") {
            return Err(invalid());
        }
        let length = word(bytes, 4).ok_or_else(invalid)? as usize;
        let (defaults, rest) = bytes[8..].split_at_checked(length).ok_or_else(invalid)?;
        let count = word(rest, 0).ok_or_else(invalid)? as usize;
        let records_length = count.checked_mul(10).ok_or_else(invalid)?;
        let (records, rest) = rest[4..]
            .split_at_checked(records_length)
            .ok_or_else(invalid)?;
        let variables = records
            .chunks_exact(10)
            .map(|entry| {
                let variable = Variable {
                    storage: entry[0],
                    persistence: entry[1],
                    offset: word(entry, 2).unwrap(),
                    bytes: word(entry, 6).unwrap(),
                };
                if variable.storage > 2
                    || variable.persistence > 2
                    || u64::from(variable.offset) + u64::from(variable.bytes) > length as u64
                {
                    return Err(invalid());
                }
                Ok(variable)
            })
            .collect::<io::Result<Vec<_>>>()?;
        let count = word(rest, 0).ok_or_else(invalid)? as usize;
        let records_length = count.checked_mul(8).ok_or_else(invalid)?;
        let (records, rest) = rest[4..]
            .split_at_checked(records_length)
            .ok_or_else(invalid)?;
        let pointers = records
            .chunks_exact(8)
            .map(|entry| {
                let address = word(entry, 0).unwrap();
                let target = word(entry, 4).unwrap();
                if u64::from(address) + 4 > length as u64 || target as usize >= length {
                    return Err(invalid());
                }
                Ok((address, target))
            })
            .collect::<io::Result<Vec<_>>>()?;
        if rest != [0; 4] {
            return Err(invalid());
        }
        Ok(Self {
            defaults: defaults.to_vec(),
            variables,
            pointers,
        })
    }
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid Lucifen SOB1 variable data",
    )
}
