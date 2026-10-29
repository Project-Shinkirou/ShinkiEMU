//! Body text: `<begin variables>` holds one native `std::map<string,string>`
//! serialized in key byte order (perseus `sub_46B930`, eden `sub_4C0810`).
//! Engine fields carry a `!` prefix (perseus `sub_45FB10`, eden `sub_4C6690`);
//! script `.set` variables live in the same map without it: perseus
//! `CommandSet` `sub_44F9D0` writes engine+6280, which save `sub_45D2D0` and
//! load `sub_45D500` pass as `a1[1570]`; eden `CommandSet` `sub_4DE550` writes
//! engine+6408, saved by `sub_4D0C50` as `a1[1602]`. A plain key whose first
//! byte is above `!` (0x21) therefore follows the whole `!` block.
use super::*;
use std::collections::BTreeSet;
const MAX_FIELDS: usize = 4096;
const MAX_RECORDS: usize = 16384;
const MAX_LINE: usize = 64 * 1024;
const MAX_NAME: usize = 128;
type Fields = Vec<EdenSaveField>;
pub(super) struct Body {
    pub variables: Fields,
    pub script_variables: Vec<(String, String)>,
    pub backlog: Vec<Fields>,
}
fn field(line: &str) -> Result<EdenSaveField, CoreError> {
    let (name, value) = line
        .split_once('\t')
        .ok_or_else(|| invalid("BODY", "save field separator is missing"))?;
    if name.is_empty()
        || name.len() > MAX_NAME
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.".contains(&b))
        || line.len() > MAX_LINE
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(invalid("BODY", "save field is invalid"));
    }
    Ok(EdenSaveField {
        name: name.into(),
        value: value.into(),
    })
}
fn push(fields: &mut Fields, names: &mut BTreeSet<String>, line: &str) -> Result<(), CoreError> {
    let field = field(line)?;
    if fields.len() >= MAX_FIELDS || !names.insert(field.name.clone()) {
        return Err(invalid(
            "BODY",
            "save fields are duplicate or exceed the count limit",
        ));
    }
    fields.push(field);
    Ok(())
}
/// Native map key bytes of a script variable. The map compares encoded bytes,
/// so ordering is decided here and not on the decoded text.
fn script_key(encoding: EdenSaveEncoding, name: &str, value: &str) -> Result<Vec<u8>, CoreError> {
    if name.len() > MAX_NAME
        || name.len() + value.len() + 1 > MAX_LINE
        || name.contains(['\0', '\t', '\n'])
        || value.contains('\0')
    {
        return Err(invalid("BODY", "save script variable is invalid"));
    }
    let key = encoding.encode(name)?;
    // An empty value erases the key in `sub_46B2D0`, so the original never
    // writes one; a key at or below `!` would precede the engine fields.
    if value.is_empty() || key.first().is_none_or(|first| *first <= b'!') {
        return Err(checkpoint::unsupported());
    }
    Ok(key)
}
pub(super) fn parse(text: &str, encoding: EdenSaveEncoding) -> Result<Body, CoreError> {
    if !text.ends_with('\n') || text.contains('\r') {
        return Err(invalid("BODY", "save body line endings are invalid"));
    }
    let mut lines = text.split_terminator('\n');
    if lines.next() != Some("<begin variables>") {
        return Err(invalid("BODY", "save variables section is missing"));
    }
    let mut variables = Vec::new();
    let mut script_variables = Vec::new();
    let mut names = BTreeSet::new();
    let mut last_key: Option<Vec<u8>> = None;
    let mut ended = false;
    for line in lines.by_ref() {
        if line == "<end variables>" {
            ended = true;
            break;
        }
        match line.strip_prefix('!') {
            Some(line) if last_key.is_none() => push(&mut variables, &mut names, line)?,
            // Plain lines and any `!` line after them: map order only.
            _ => {
                let (name, value) = line.split_once('\t').ok_or_else(checkpoint::unsupported)?;
                let key = script_key(encoding, name, value)?;
                if last_key.as_ref().is_some_and(|last| *last >= key) {
                    return Err(checkpoint::unsupported());
                }
                if script_variables.len() >= MAX_FIELDS {
                    return Err(invalid(
                        "BOUND",
                        "save script variable count exceeds the limit",
                    ));
                }
                script_variables.push((name.to_owned(), value.to_owned()));
                last_key = Some(key);
            }
        }
    }
    if !ended || variables.is_empty() || lines.next() != Some("<<begin backlog>>") {
        return Err(invalid("BODY", "save sections are missing or out of order"));
    }
    let mut backlog = Vec::new();
    let mut record = Vec::new();
    names.clear();
    ended = false;
    for line in lines.by_ref() {
        if line == "<<end backlog>>" {
            ended = true;
            break;
        }
        if line == "ZZ" {
            if record.is_empty() || backlog.len() >= MAX_RECORDS {
                return Err(invalid("BOUND", "save backlog record count is invalid"));
            }
            backlog.push(std::mem::take(&mut record));
            names.clear();
        } else {
            push(&mut record, &mut names, line)?;
        }
    }
    if !ended || !record.is_empty() || lines.next().is_some() {
        return Err(invalid(
            "BODY",
            "save backlog is incomplete or has trailing data",
        ));
    }
    Ok(Body {
        variables,
        script_variables,
        backlog,
    })
}
fn append(text: &mut String, name: &str, value: &str, prefix: &str) -> Result<(), CoreError> {
    let length = name.len().saturating_add(value.len()).saturating_add(2);
    if length > MAX_LINE
        || text
            .len()
            .saturating_add(length)
            .saturating_add(prefix.len())
            > MAX_BODY
    {
        return Err(invalid(
            "BOUND",
            "save body or field exceeds the byte limit",
        ));
    }
    text.push_str(prefix);
    text.push_str(name);
    text.push('\t');
    text.push_str(value);
    text.push('\n');
    Ok(())
}
pub(super) fn format(save: &EdenSave) -> Result<String, CoreError> {
    if save.variables.len() > MAX_FIELDS
        || save.script_variables.len() > MAX_FIELDS
        || save.backlog.len() > MAX_RECORDS
        || save.backlog.iter().any(|record| record.len() > MAX_FIELDS)
    {
        return Err(invalid(
            "BOUND",
            "save field or record count exceeds the limit",
        ));
    }
    let mut script_variables = save
        .script_variables
        .iter()
        .map(|(name, value)| Ok((script_key(save.encoding, name, value)?, (name, value))))
        .collect::<Result<Vec<_>, CoreError>>()?;
    script_variables.sort_by(|left, right| left.0.cmp(&right.0));
    if script_variables
        .windows(2)
        .any(|pair| pair[0].0 == pair[1].0)
    {
        return Err(checkpoint::unsupported());
    }
    let mut text = String::from("<begin variables>\n");
    for item in &save.variables {
        append(&mut text, &item.name, &item.value, "!")?;
    }
    for (_, (name, value)) in &script_variables {
        append(&mut text, name, value, "")?;
    }
    text.push_str("<end variables>\n<<begin backlog>>\n");
    for fields in &save.backlog {
        for item in fields {
            append(&mut text, &item.name, &item.value, "")?;
        }
        text.push_str("ZZ\n");
    }
    text.push_str("<<end backlog>>\n");
    let body = parse(&text, save.encoding)?;
    if body.variables != save.variables
        || body.backlog != save.backlog
        || !body
            .script_variables
            .iter()
            .map(|(name, value)| (name, value))
            .eq(script_variables.iter().map(|(_, pair)| *pair))
    {
        return Err(invalid(
            "BODY",
            "save field contains a structural delimiter",
        ));
    }
    Ok(text)
}
