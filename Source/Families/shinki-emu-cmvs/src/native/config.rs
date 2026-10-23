use super::*;
use shinki_emu_sdk::runtime;
use std::sync::LazyLock;
struct Schema {
    descriptor: abi::Descriptor,
    _source: Vec<runtime::ConfigField>,
    _fields: Vec<abi::ConfigField>,
    _formats: Vec<abi::Bytes>,
    _choices: Vec<Vec<abi::Bytes>>,
}
// All pointers refer to immutable module-lifetime allocations built once.
unsafe impl Send for Schema {}
unsafe impl Sync for Schema {}
const CAPABILITIES: u64 = abi::CAP_CPU_SURFACE;
static SCHEMA: LazyLock<Schema> = LazyLock::new(|| {
    let source = crate::family::cmvs_descriptor().configuration;
    let mut all_choices = Vec::new();
    let mut fields = Vec::with_capacity(source.len());
    for field in &source {
        let mut target = abi::ConfigField {
            id: bytes(&field.id),
            label: bytes(&field.label),
            group: bytes(&field.group),
            default_value: value(&field.default),
            integer_min: 0,
            integer_max: 0,
            number_min: 0.0,
            number_max: 0.0,
            max_bytes: 0,
            flags: abi::CONFIG_FLAG_FILE_PATH | abi::CONFIG_FLAG_GAME_RELATIVE,
            choices: std::ptr::null(),
            choice_count: 0,
            description: abi::Bytes::EMPTY,
            choice_labels: std::ptr::null(),
            step: 0.0,
            unit: abi::Bytes::EMPTY,
        };
        match &field.kind {
            runtime::ConfigKind::Bool => {}
            runtime::ConfigKind::Integer { min, max } => {
                target.integer_min = *min;
                target.integer_max = *max;
            }
            runtime::ConfigKind::Number { min, max } => {
                target.number_min = *min;
                target.number_max = *max;
            }
            runtime::ConfigKind::String { max_bytes } => target.max_bytes = *max_bytes,
            runtime::ConfigKind::Enum { choices } => {
                let choices: Vec<_> = choices.iter().map(|v| bytes(v)).collect();
                let labels: Vec<_> = choices.to_vec();
                target.choices = choices.as_ptr();
                target.choice_count = choices.len();
                target.choice_labels = labels.as_ptr();
                all_choices.push(choices);
                all_choices.push(labels);
            }
        }
        fields.push(target);
    }
    let formats: Vec<_> = ["cmvs.cpz5"].iter().map(|v| bytes(v)).collect();
    Schema {
        descriptor: abi::Descriptor {
            header: abi::Header::of::<abi::Descriptor>(),
            family_id: bytes("cmvs"),
            plugin_id: bytes("shinki.emu.cmvs"),
            version: bytes(env!("CARGO_PKG_VERSION")),
            capabilities: CAPABILITIES,
            formats: formats.as_ptr(),
            format_count: formats.len(),
            configuration: fields.as_ptr(),
            configuration_count: fields.len(),
            display_name: bytes("Shinki CMVS"),
            family_name: bytes("CMVS"),
            render: abi::RenderExtentSupport::default(),
        },
        _source: source,
        _fields: fields,
        _formats: formats,
        _choices: all_choices,
    }
});
pub(super) fn descriptor() -> abi::Descriptor {
    SCHEMA.descriptor
}
pub(super) fn render_extent(request: &abi::OpenRequest) -> Result<abi::Extent, Failure> {
    abi::RenderExtentSupport::default()
        .validate_request(CAPABILITIES, request.render_extent)
        .map_err(|status| {
            Failure::new(
                status,
                "SHINKI_EMU_CMVS_RENDER_EXTENT",
                "render extent is outside the supported range",
            )
        })?;
    Ok(request.render_extent)
}
fn bytes(value: &str) -> abi::Bytes {
    abi::Bytes::new(value.as_bytes())
}
fn value(value: &runtime::ConfigValue) -> abi::ConfigValue {
    let mut target = abi::ConfigValue {
        kind: 0,
        reserved: 0,
        integer: 0,
        number: 0.0,
        text: abi::Bytes::EMPTY,
    };
    match value {
        runtime::ConfigValue::Bool(value) => {
            target.kind = abi::VALUE_BOOL;
            target.integer = i64::from(*value);
        }
        runtime::ConfigValue::Integer(value) => {
            target.kind = abi::VALUE_INTEGER;
            target.integer = *value;
        }
        runtime::ConfigValue::Number(value) => {
            target.kind = abi::VALUE_NUMBER;
            target.number = *value;
        }
        runtime::ConfigValue::String(value) => {
            target.kind = abi::VALUE_STRING;
            target.text = bytes(value);
        }
        runtime::ConfigValue::Enum(value) => {
            target.kind = abi::VALUE_ENUM;
            target.text = bytes(value);
        }
    }
    target
}
pub(super) unsafe fn entries(
    request: &abi::OpenRequest,
) -> Result<Vec<runtime::ConfigEntry>, Failure> {
    unsafe { shinki_emu_sdk::native::config_entries(descriptor(), request) }.map_err(invalid)
}
