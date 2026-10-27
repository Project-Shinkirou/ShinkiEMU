use super::*;
use std::borrow::Cow;

// `sub_4184B0`'s CP932 table, ASCII 0x20..0x7f (last entry is unused).
const FULLWIDTH_ASCII: [u16; 96] = [
    0x8140, 0x8149, 0x8168, 0x8194, 0x8190, 0x8193, 0x8195, 0x8166, 0x8169, 0x816A, 0x8196, 0x817B,
    0x8143, 0x8151, 0x8144, 0x815E, 0x824F, 0x8250, 0x8251, 0x8252, 0x8253, 0x8254, 0x8255, 0x8256,
    0x8257, 0x8258, 0x8146, 0x8147, 0x8183, 0x8181, 0x8184, 0x8148, 0x8197, 0x8260, 0x8261, 0x8262,
    0x8263, 0x8264, 0x8265, 0x8266, 0x8267, 0x8268, 0x8269, 0x826A, 0x826B, 0x826C, 0x826D, 0x826E,
    0x826F, 0x8270, 0x8271, 0x8272, 0x8273, 0x8274, 0x8275, 0x8276, 0x8277, 0x8278, 0x8279, 0x816D,
    0x818F, 0x816E, 0x814F, 0x8151, 0x814D, 0x8281, 0x8282, 0x8283, 0x8284, 0x8285, 0x8286, 0x8287,
    0x8288, 0x8289, 0x828A, 0x828B, 0x828C, 0x828D, 0x828E, 0x828F, 0x8290, 0x8291, 0x8292, 0x8293,
    0x8294, 0x8295, 0x8296, 0x8297, 0x8298, 0x8299, 0x829A, 0x816F, 0x8162, 0x8170, 0x8150, 0x8140,
];

/// One-word caller of `sub_418D70`: type, optional sign, then up to two
/// width digits. `%z3a` is zero-padded decimal followed by literal `a`.
pub(super) fn render<'a>(
    format: &[u8],
    argument: u32,
    mut text: impl FnMut() -> Result<Cow<'a, [u8]>, CoreError>,
) -> Result<Vec<u8>, CoreError> {
    let mut output = Vec::with_capacity(format.len() + 16);
    let mut index = 0;
    let mut consumed = false;
    while index < format.len() {
        if format[index] != b'%' {
            output.push(format[index]);
            index += 1;
            continue;
        }
        index += 1;
        let Some(&kind) = format.get(index) else {
            break;
        };
        index += 1;
        if kind == b'%' {
            output.push(b'%');
            continue;
        }
        let kind = if kind == b'l' {
            let Some(&kind) = format.get(index) else {
                break;
            };
            index += 1;
            if !matches!(kind, b'd' | b'D' | b'u' | b'U' | b'x' | b'X' | b'o' | b'O') {
                continue;
            }
            kind
        } else {
            kind
        };
        if matches!(kind, b'f' | b'F' | b't' | b'T' | b'r') {
            return Err(invalid(
                "SHINKI_EMU_CMVS_FORMAT_ARGUMENT",
                "directive requires a wider or destination-relative argument",
            ));
        }
        if !matches!(
            kind,
            b'd' | b'D'
                | b'u'
                | b'U'
                | b'z'
                | b'Z'
                | b'x'
                | b'X'
                | b'o'
                | b'O'
                | b's'
                | b'S'
                | b'c'
        ) {
            // Native unknown directives emit neither the percent nor type.
            continue;
        }
        if consumed {
            return Err(invalid(
                "SHINKI_EMU_CMVS_FORMAT_ARGUMENT",
                "format consumes more than the supplied word",
            ));
        }
        consumed = true;
        if kind == b'c' {
            output.push(argument as u8);
            continue;
        }
        let wide = kind.is_ascii_uppercase();
        let kind = kind.to_ascii_lowercase();
        let sign = if kind != b's' && matches!(format.get(index), Some(b'+' | b'-')) {
            let sign = format[index];
            index += 1;
            Some(sign)
        } else {
            None
        };
        let mut width = 0_usize;
        for _ in 0..2 {
            let Some(&digit) = format.get(index).filter(|digit| digit.is_ascii_digit()) else {
                break;
            };
            width = width * 10 + usize::from(digit - b'0');
            index += 1;
        }
        if kind == b's' {
            let value = text()?;
            let start = output.len();
            if wide {
                append_fullwidth(&mut output, &value);
            } else {
                output.extend_from_slice(&value);
            }
            if width != 0 {
                let end = start + width * if wide { 2 } else { 1 };
                output.truncate(end);
                output.resize(end, b' ');
            }
        } else {
            append_number(&mut output, argument, kind, width, sign, wide)?;
        }
    }
    if let Some(nul) = output.iter().position(|byte| *byte == 0) {
        output.truncate(nul);
    }
    Ok(output)
}

fn append_number(
    output: &mut Vec<u8>,
    argument: u32,
    kind: u8,
    width: usize,
    sign: Option<u8>,
    wide: bool,
) -> Result<(), CoreError> {
    let value = if kind == b'd' {
        (argument as i32).wrapping_abs() as u32
    } else {
        argument
    };
    let radix = match kind {
        b'x' => 16,
        b'o' => 8,
        _ => 10,
    };
    let alphabet = b"0123456789ABCDEF";
    let mut buffer = [b' '; 100];
    let mut start = buffer.len();
    if width == 0 {
        if matches!(kind, b'z' | b'x' | b'o') {
            return Ok(());
        }
        let mut value = value;
        loop {
            start -= 1;
            buffer[start] = alphabet[(value % 10) as usize];
            value /= 10;
            if value == 0 {
                break;
            }
        }
    } else {
        start -= width;
        let mut divisor = 1_u32;
        for position in (start..buffer.len()).rev() {
            if divisor == 0 {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_FORMAT_WIDTH",
                    "native digit divisor overflowed to zero",
                ));
            }
            buffer[position] = if value < divisor {
                if matches!(kind, b'z' | b'x' | b'o') {
                    b'0'
                } else {
                    b' '
                }
            } else {
                alphabet[((value / divisor) % radix) as usize]
            };
            divisor = divisor.wrapping_mul(radix);
        }
        if value == 0 {
            *buffer.last_mut().unwrap() = b'0';
        }
    }
    if kind == b'd'
        && let Some(flag) = sign
    {
        let sign = if (argument as i32) < 0 {
            b'-'
        } else if flag == b'+' {
            b'+'
        } else {
            b' '
        };
        if width == 0 {
            if sign != b' ' {
                start -= 1;
                buffer[start] = sign;
            }
        } else if let Some(first) = buffer[start..].iter().position(|byte| *byte != b' ')
            && first != 0
        {
            buffer[start + first - 1] = sign;
        }
    }
    if wide {
        append_fullwidth(output, &buffer[start..]);
    } else {
        output.extend_from_slice(&buffer[start..]);
    }
    Ok(())
}

fn append_fullwidth(output: &mut Vec<u8>, bytes: &[u8]) {
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if matches!(byte, 0x81..=0x9f | 0xe0..=0xfc) && index + 1 < bytes.len() {
            output.extend_from_slice(&bytes[index..index + 2]);
            index += 2;
        } else {
            if (0x20..=0x7e).contains(&byte) {
                output.extend_from_slice(&FULLWIDTH_ASCII[usize::from(byte - 0x20)].to_be_bytes());
            } else {
                output.push(byte);
            }
            index += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn number(format: &[u8], value: u32) -> Vec<u8> {
        render(format, value, || {
            unreachable!("integer directive resolved a string")
        })
        .unwrap()
    }
    #[test]
    fn native_widths_truncate_and_signs_are_explicit() {
        assert_eq!(number(b"%z3", 12345), b"345");
        assert_eq!(number(b"%d3", (-5_i32) as u32), b"  5");
        assert_eq!(number(b"%d+3", (-5_i32) as u32), b" -5");
        assert_eq!(number(b"%x3", 0x12ab), b"2AB");
        assert_eq!(number(b"%o3", 9), b"011");
        assert_eq!(number(b"%%-%u", 12), b"%-12");
    }
    #[test]
    fn upper_case_converts_to_native_full_width_cp932() {
        assert_eq!(number(b"%Z3", 2), vec![0x82, 0x4f, 0x82, 0x4f, 0x82, 0x51]);
        assert_eq!(
            number(b"%D+3", (-5_i32) as u32),
            vec![0x81, 0x40, 0x81, 0x51, 0x82, 0x54]
        );
    }
    #[test]
    fn string_width_is_native_bytes_not_unicode_characters() {
        let text = [0x82, 0xa0, b'B'];
        assert_eq!(
            render(b"[%s2]", 0, || Ok(Cow::Borrowed(&text))).unwrap(),
            [b'[', 0x82, 0xa0, b']']
        );
        assert_eq!(
            render(b"[%S3]", 0, || Ok(Cow::Borrowed(b"A"))).unwrap(),
            [b"[".as_slice(), &[0x82, 0x60], b"    ]".as_slice()].concat()
        );
        assert_eq!(number(b"%c", 0), b"");
    }
    #[test]
    fn missing_varargs_are_rejected_instead_of_reading_beyond_the_stack_word() {
        assert_eq!(
            render(b"%u %u", 4, || Ok(Cow::Borrowed(b"")))
                .unwrap_err()
                .code(),
            "SHINKI_EMU_CMVS_FORMAT_ARGUMENT"
        );
    }
}
