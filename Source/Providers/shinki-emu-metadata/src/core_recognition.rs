//! Data-only detection clauses carried by an official core recipe and package.
use serde::{Deserialize, Serialize};

const MAX_RULES: usize = 4;
const MAX_PATH_BYTES: usize = 256;
const MAX_COMPONENTS: usize = 4;
const MAX_MAGIC_BYTES: usize = 32;
const MAX_MAGIC_END: u64 = 4096;

/// An OR-list of exact relative file paths, optionally constrained by magic bytes.
/// The enclosing package/catalog entry supplies the plugin and family identity.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<CoreRecognitionRule>")]
pub struct CoreRecognition(Vec<CoreRecognitionRule>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreRecognitionRule {
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    magic: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    offset: u64,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

impl CoreRecognition {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn rules(&self) -> &[CoreRecognitionRule] {
        &self.0
    }
}

impl CoreRecognitionRule {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    pub fn magic_bytes(&self) -> Vec<u8> {
        self.magic.as_ref().map_or_else(Vec::new, |magic| {
            hex::decode(magic).expect("validated recognition magic")
        })
    }
}

impl TryFrom<Vec<CoreRecognitionRule>> for CoreRecognition {
    type Error = &'static str;

    fn try_from(mut rules: Vec<CoreRecognitionRule>) -> Result<Self, Self::Error> {
        if rules.len() > MAX_RULES {
            return Err("too many core recognition rules");
        }
        for rule in &mut rules {
            if rule.path.len() > MAX_PATH_BYTES
                || rule.path.split('/').count() > MAX_COMPONENTS
                || rule.path.split('/').any(|part| {
                    part.is_empty()
                        || part.starts_with('.')
                        || part.ends_with(['.', ' '])
                        || !part
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"._- ".contains(&b))
                })
            {
                return Err("invalid core recognition path");
            }
            rule.path.make_ascii_lowercase();
            if let Some(magic) = &mut rule.magic {
                if magic.is_empty()
                    || magic.len() % 2 != 0
                    || magic.len() > MAX_MAGIC_BYTES * 2
                    || !magic.bytes().all(|b| b.is_ascii_hexdigit())
                    || rule
                        .offset
                        .checked_add((magic.len() / 2) as u64)
                        .is_none_or(|end| end > MAX_MAGIC_END)
                {
                    return Err("invalid core recognition magic or offset");
                }
                magic.make_ascii_lowercase();
            } else if rule.offset != 0 {
                return Err("core recognition offset requires magic");
            }
        }
        rules.sort_unstable_by(|a, b| {
            (&a.path, a.offset, &a.magic).cmp(&(&b.path, b.offset, &b.magic))
        });
        rules.dedup();
        Ok(Self(rules))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn confines_paths_and_magic_before_matching() {
        for value in [
            json!([{"path":"../outside"}]),
            json!([{"path":"a//b"}]),
            json!([{"path":"a\\b"}]),
            json!([{"path":"C:/b"}]),
            json!([{"path":".shinki/b"}]),
            json!([{"path":"a/b/c/d/e"}]),
            json!([{"path": "x".repeat(257)}]),
            json!([{"path":"a", "magic":""}]),
            json!([{"path":"a", "magic":"zz"}]),
            json!([{"path":"a", "magic":"abcd", "offset":4095}]),
            json!([{"path":"a", "magic":"00", "offset":u64::MAX}]),
            json!([{"path":"a", "magic":"00".repeat(33)}]),
            json!([{"path":"a", "offset":2}]),
            json!([{"path":"a", "script":"x"}]),
            json!(vec![json!({"path":"a"}); 5]),
        ] {
            assert!(
                serde_json::from_value::<CoreRecognition>(value.clone()).is_err(),
                "{value}"
            );
        }
        let normalized: CoreRecognition = serde_json::from_value(json!([
            {"path":"Data/Boot.bin", "offset":4094, "magic":"DeAD"},
            {"path":"data/boot.bin", "offset":4094, "magic":"dead"}
        ]))
        .unwrap();
        assert_eq!(normalized.rules().len(), 1);
        assert_eq!(normalized.rules()[0].path(), "data/boot.bin");
        assert_eq!(normalized.rules()[0].magic_bytes(), [0xde, 0xad]);
    }
}
