use super::*;

fn fields() -> Vec<ConfigField> {
    vec![
        ("enabled", ConfigKind::Bool, ConfigValue::Bool(true)),
        (
            "count",
            ConfigKind::Integer { min: -4, max: 9 },
            ConfigValue::Integer(2),
        ),
        (
            "gain",
            ConfigKind::Number { min: 0.0, max: 1.0 },
            ConfigValue::Number(0.5),
        ),
        (
            "name",
            ConfigKind::String { max_bytes: 6 },
            ConfigValue::String("日本".into()),
        ),
        (
            "mode",
            ConfigKind::Enum {
                choices: vec!["a".into(), "b".into()],
            },
            ConfigValue::Enum("a".into()),
        ),
    ]
    .into_iter()
    .map(|(id, kind, default)| ConfigField {
        id: id.into(),
        label: id.into(),
        group: "General".into(),
        kind,
        default,
    })
    .collect()
}

#[test]
fn resolves_all_types_and_schema_order_with_defaults() {
    let fields = fields();
    let resolved = resolve_config(
        &fields,
        &[ConfigEntry {
            id: "mode".into(),
            value: ConfigValue::Enum("b".into()),
        }],
    )
    .unwrap();
    assert_eq!(resolved.len(), 5);
    assert_eq!(resolved[0].value, ConfigValue::Bool(true));
    assert_eq!(resolved[3].value, ConfigValue::String("日本".into()));
    assert_eq!(resolved[4].value, ConfigValue::Enum("b".into()));
}

#[test]
fn rejects_unknown_duplicate_mismatched_and_out_of_bounds_values() {
    let schema = fields();
    for (id, value) in [
        ("unknown", ConfigValue::Bool(false)),
        ("enabled", ConfigValue::Integer(1)),
        ("count", ConfigValue::Integer(10)),
        ("gain", ConfigValue::Number(f64::NAN)),
        ("gain", ConfigValue::Number(f64::INFINITY)),
        ("name", ConfigValue::String("日本語".into())),
        ("name", ConfigValue::String("\0".into())),
        ("mode", ConfigValue::Enum("c".into())),
        ("mode", ConfigValue::String("a".into())),
    ] {
        assert!(
            resolve_config(
                &schema,
                &[ConfigEntry {
                    id: id.into(),
                    value
                }]
            )
            .is_err()
        );
    }
    let entry = ConfigEntry {
        id: "count".into(),
        value: ConfigValue::Integer(3),
    };
    assert!(resolve_config(&schema, &[entry.clone(), entry]).is_err());
}

#[test]
fn rejects_invalid_schema_even_when_no_values_are_supplied() {
    let schema = fields();
    let mut duplicate = schema.clone();
    duplicate.push(schema[0].clone());
    assert!(resolve_config(&duplicate, &[]).is_err());
    for kind in [
        ConfigKind::Integer { min: 9, max: 2 },
        ConfigKind::Number {
            min: f64::NEG_INFINITY,
            max: 1.0,
        },
        ConfigKind::Enum {
            choices: vec!["a".into(), "a".into()],
        },
        ConfigKind::String {
            max_bytes: MAX_CONFIG_TEXT_BYTES as u32 + 1,
        },
    ] {
        let mut field = schema[0].clone();
        field.kind = kind;
        assert!(resolve_config(&[field], &[]).is_err());
    }
    let mut field = schema[0].clone();
    field.default = ConfigValue::Integer(1);
    assert!(resolve_config(&[field], &[]).is_err());
    assert!(resolve_config(&vec![schema[0].clone(); MAX_CONFIG_FIELDS + 1], &[]).is_err());
}
