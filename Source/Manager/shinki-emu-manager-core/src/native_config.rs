use super::family_loader::text;
use crate::{
    FamilyCapability, FamilyPluginDescriptor, INDEPENDENT_FAMILY_ABI_FINGERPRINT, RenderSupport,
    model::*,
};
use shinki_emu_abi as abi;
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Debug)]
pub struct ConfigFileFilter {
    pub label: String,
    pub extensions: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ConfigPathOptions {
    pub filter: Option<ConfigFileFilter>,
    pub validates_path: bool,
}

pub(super) struct ConfigPathRule {
    pub(super) options: ConfigPathOptions,
    validate: Option<unsafe extern "C" fn(abi::Bytes, *mut abi::Error) -> abi::Status>,
}

impl ConfigPathRule {
    pub(super) fn validate(&self, path: &Path) -> FamilyResult<()> {
        let validate = self.validate.ok_or_else(|| {
            FamilyError::invalid("SHINKI_EMU_CORE_CONFIG", "no validator for this path field")
        })?;
        let path = path
            .to_str()
            .filter(|text| {
                path.is_absolute()
                    && text.len() <= abi::MAX_CONFIG_PATH_BYTES
                    && !text.contains('\0')
            })
            .ok_or_else(|| {
                FamilyError::invalid("SHINKI_EMU_CONFIG_PATH_INVALID", "invalid native path")
            })?;
        let mut error = abi::Error::default();
        let status = unsafe { validate(abi::Bytes::new(path.as_bytes()), &mut error) };
        super::family_loader::checked("validate_config_path", status, error)
    }
}

/// Reads the optional table on the owner. Only owned metadata leaves it.
pub(super) fn path_rules(
    api: &abi::CoreApi,
    schema: &[ConfigField],
) -> Result<BTreeMap<String, ConfigPathRule>, crate::FamilyLoadError> {
    use crate::FamilyLoadError::AbiLayout;

    let mut raw = std::ptr::null();
    let status = unsafe {
        api.extension.unwrap()(
            abi::Bytes::new(abi::CONFIG_PATHS_EXTENSION),
            abi::CONFIG_PATHS_VERSION,
            &mut raw,
        )
    };
    if status == abi::UNSUPPORTED && raw.is_null() {
        return Ok(BTreeMap::new());
    }
    let table = raw.cast::<abi::ConfigPathsV1>();
    if status != abi::OK || table.is_null() || !table.is_aligned() {
        return Err(AbiLayout);
    }
    let header = unsafe { *raw.cast::<abi::Header>() };
    if header.version != abi::CONFIG_PATHS_VERSION || header.size < abi::ConfigPathsV1::HEADER.size
    {
        return Err(AbiLayout);
    }
    let table = unsafe { &*table };
    let fields = unsafe { abi::borrowed_array(table.fields, table.count, schema.len()) }
        .map_err(|_| AbiLayout)?;
    let mut rules = BTreeMap::new();
    for field in fields {
        let id = unsafe { field.field_id.text(128) }.map_err(|_| AbiLayout)?;
        let declared = schema.iter().find(|item| item.id == id).ok_or(AbiLayout)?;
        if !declared.flags.file_path && !declared.flags.directory_path {
            return Err(AbiLayout);
        }
        let label = unsafe {
            field
                .filter_label
                .text(abi::MAX_CONFIG_FILE_FILTER_LABEL_BYTES)
        }
        .map_err(|_| AbiLayout)?;
        let filter = if field.extension_count == 0 {
            if !label.is_empty() {
                return Err(AbiLayout);
            }
            None
        } else {
            if !declared.flags.file_path
                || label.trim().is_empty()
                || label.chars().any(char::is_control)
            {
                return Err(AbiLayout);
            }
            let raw = unsafe {
                abi::borrowed_array(
                    field.extensions,
                    field.extension_count,
                    abi::MAX_CONFIG_FILE_EXTENSIONS,
                )
            }
            .map_err(|_| AbiLayout)?;
            let mut extensions: Vec<String> = Vec::with_capacity(raw.len());
            for bytes in raw {
                let extension = unsafe { bytes.text(abi::MAX_CONFIG_FILE_EXTENSION_BYTES) }
                    .map_err(|_| AbiLayout)?;
                if extension.is_empty()
                    || !extension.bytes().all(|b| b.is_ascii_alphanumeric())
                    || extensions
                        .iter()
                        .any(|entry| entry.eq_ignore_ascii_case(extension))
                {
                    return Err(AbiLayout);
                }
                extensions.push(extension.to_owned());
            }
            Some(ConfigFileFilter {
                label: label.to_owned(),
                extensions,
            })
        };
        let rule = ConfigPathRule {
            options: ConfigPathOptions {
                filter,
                validates_path: field.validate.is_some(),
            },
            validate: field.validate,
        };
        if rules.insert(id.to_owned(), rule).is_some() {
            return Err(AbiLayout);
        }
    }
    Ok(rules)
}

pub(super) unsafe fn descriptor(
    raw: abi::Descriptor,
) -> FamilyResult<(FamilyPluginDescriptor, Vec<ConfigField>)> {
    let invalid = || FamilyError::invalid("SHINKI_EMU_CORE_DESCRIPTOR", "invalid core descriptor");
    if unsafe { raw.validate() }.is_err() {
        return Err(invalid());
    }
    let formats = unsafe { abi::borrowed_array(raw.formats, raw.format_count, 128) }
        .map_err(|_| invalid())?;
    let fields = unsafe {
        abi::borrowed_array(
            raw.configuration,
            raw.configuration_count,
            abi::MAX_CONFIG_FIELDS,
        )
    }
    .map_err(|_| invalid())?;
    unsafe { abi::validate_config(fields, &[]) }.map_err(|_| invalid())?;
    let mut configuration = Vec::with_capacity(fields.len());
    for field in fields {
        let default = unsafe { value(field.default_value) }?;
        let kind = match field.default_value.kind {
            abi::VALUE_BOOL => ConfigKind::Bool,
            abi::VALUE_INTEGER => ConfigKind::Integer {
                min: field.integer_min,
                max: field.integer_max,
            },
            abi::VALUE_NUMBER => ConfigKind::Number {
                min: field.number_min,
                max: field.number_max,
            },
            abi::VALUE_STRING => ConfigKind::String {
                max_bytes: field.max_bytes,
            },
            abi::VALUE_ENUM => ConfigKind::Enum {
                choices: unsafe { texts(field.choices, field.choice_count, 128) }?,
            },
            _ => return Err(invalid()),
        };
        let choice_labels = if field.choice_count == 0 {
            Vec::new()
        } else {
            unsafe {
                texts(
                    field.choice_labels,
                    field.choice_count,
                    abi::MAX_CHOICE_LABEL,
                )
            }?
        };
        configuration.push(ConfigField {
            id: unsafe { text(field.id, 128) }?,
            label: unsafe { text(field.label, 256) }?,
            group: unsafe { text(field.group, 256) }?,
            kind,
            default,
            description: unsafe { text(field.description, abi::MAX_CONFIG_DESCRIPTION) }?,
            choice_labels,
            flags: ConfigFlags {
                file_path: field.flags & abi::CONFIG_FLAG_FILE_PATH != 0,
                directory_path: field.flags & abi::CONFIG_FLAG_DIRECTORY_PATH != 0,
                advanced: field.flags & abi::CONFIG_FLAG_ADVANCED != 0,
                game_relative: field.flags & abi::CONFIG_FLAG_GAME_RELATIVE != 0,
            },
            step: (field.step != 0.0).then_some(field.step),
            unit: unsafe { text(field.unit, abi::MAX_CONFIG_UNIT) }?,
        });
    }
    let mut capabilities = Vec::new();
    if raw.capabilities & abi::CAP_CPU_SURFACE != 0 {
        capabilities.push(FamilyCapability::CpuFrame);
    }
    if raw.capabilities & abi::CAP_RENDER_EXTENT != 0 {
        capabilities.push(FamilyCapability::RenderExtent);
    }
    if raw.capabilities & abi::CAP_TEXT_HOOK != 0 {
        capabilities.push(FamilyCapability::TextReplacement);
    }
    if raw.capabilities & abi::CAP_ASYNC_TEXT != 0 {
        capabilities.push(FamilyCapability::AsyncTextReplacement);
    }
    if raw.capabilities & abi::CAP_NATIVE_SCRIPT_IDENTITY != 0 {
        capabilities.push(FamilyCapability::NativeScriptIdentity);
    }
    if raw.capabilities & abi::CAP_CONFIGURED_FONT_PRIORITY != 0 {
        capabilities.push(FamilyCapability::ConfiguredFontPriority);
    }
    if raw.capabilities & abi::CAP_NATIVE_EMBEDDED_TEXT != 0 {
        capabilities.push(FamilyCapability::NativeEmbeddedText);
    }
    if raw.capabilities & abi::CAP_NATIVE_NAME_RESOURCES != 0 {
        capabilities.push(FamilyCapability::NativeNameResources);
    }
    if raw.capabilities & abi::CAP_AUTHORED_GALLERY_CLIP != 0 {
        capabilities.push(FamilyCapability::AuthoredGalleryClip);
    }
    let descriptor = FamilyPluginDescriptor {
        family_id: unsafe { text(raw.family_id, 256) }?,
        plugin_id: unsafe { text(raw.plugin_id, 256) }?,
        version: unsafe { text(raw.version, 256) }?,
        abi_fingerprint: INDEPENDENT_FAMILY_ABI_FINGERPRINT.into(),
        capabilities,
        supported_formats: formats
            .iter()
            .map(|&v| unsafe { text(v, 256) })
            .collect::<FamilyResult<_>>()?,
        display_name: unsafe { text(raw.display_name, abi::MAX_DISPLAY_NAME_BYTES) }?,
        family_name: unsafe { text(raw.family_name, abi::MAX_DISPLAY_NAME_BYTES) }?,
        render: (raw.capabilities & abi::CAP_RENDER_EXTENT != 0).then(|| {
            let extent = |value: abi::Extent| (value.width, value.height);
            RenderSupport {
                min: extent(raw.render.min),
                max: extent(raw.render.max),
                default: (raw.render.default != abi::Extent::default())
                    .then(|| extent(raw.render.default)),
                preserve_aspect: raw.render.flags & abi::RENDER_PRESERVE_ASPECT != 0,
            }
        }),
    };
    descriptor.validate().map_err(|_| invalid())?;
    validate_config_schema(&configuration)?;
    Ok((descriptor, configuration))
}
unsafe fn texts(items: *const abi::Bytes, count: usize, limit: usize) -> FamilyResult<Vec<String>> {
    unsafe { abi::borrowed_array(items, count, abi::MAX_CONFIG_FIELDS) }
        .map_err(|_| FamilyError::invalid("SHINKI_EMU_CORE_DESCRIPTOR", "invalid core descriptor"))?
        .iter()
        .map(|&value| unsafe { text(value, limit) })
        .collect()
}
unsafe fn value(value: abi::ConfigValue) -> FamilyResult<ConfigValue> {
    Ok(match value.kind {
        abi::VALUE_BOOL => ConfigValue::Bool(value.integer != 0),
        abi::VALUE_INTEGER => ConfigValue::Integer(value.integer),
        abi::VALUE_NUMBER => ConfigValue::Number(value.number),
        abi::VALUE_STRING => {
            ConfigValue::String(unsafe { text(value.text, abi::MAX_CONFIG_TEXT) }?)
        }
        abi::VALUE_ENUM => ConfigValue::Enum(unsafe { text(value.text, 128) }?),
        _ => {
            return Err(FamilyError::invalid(
                "SHINKI_EMU_CORE_CONFIG",
                "unknown configuration type",
            ));
        }
    })
}
pub(super) fn entries(entries: &[ConfigEntry]) -> Vec<abi::ConfigEntry> {
    entries
        .iter()
        .map(|entry| {
            let mut value = abi::ConfigValue {
                kind: 0,
                reserved: 0,
                integer: 0,
                number: 0.0,
                text: abi::Bytes::EMPTY,
            };
            match &entry.value {
                ConfigValue::Bool(v) => {
                    value.kind = abi::VALUE_BOOL;
                    value.integer = i64::from(*v);
                }
                ConfigValue::Integer(v) => {
                    value.kind = abi::VALUE_INTEGER;
                    value.integer = *v;
                }
                ConfigValue::Number(v) => {
                    value.kind = abi::VALUE_NUMBER;
                    value.number = *v;
                }
                ConfigValue::String(v) => {
                    value.kind = abi::VALUE_STRING;
                    value.text = abi::Bytes::new(v.as_bytes());
                }
                ConfigValue::Enum(v) => {
                    value.kind = abi::VALUE_ENUM;
                    value.text = abi::Bytes::new(v.as_bytes());
                }
            }
            abi::ConfigEntry {
                id: abi::Bytes::new(entry.id.as_bytes()),
                value,
            }
        })
        .collect()
}
