//! Architecture mapping shared by the OHOS build scripts and HAP packager.
use std::path::{Path, PathBuf};

pub const MANAGER_LIBRARY: &str = "libshinki_emu_manager.so";

/// The single companion DSO in a Manager HAP, on each supported OHOS ABI.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HapCore {
    pub library: String,
}

impl HapCore {
    pub fn validate(&self, id: &str) -> anyhow::Result<()> {
        let valid_id = !matches!(id, "" | "." | ".." | "manager")
            && id.len() <= 128
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
        let valid_library = self.library != MANAGER_LIBRARY
            && self.library
                .strip_prefix("libshinki_emu_")
                .and_then(|name| name.strip_suffix(".so"))
                .is_some_and(|name| {
                    !name.is_empty()
                        && name
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                });
        anyhow::ensure!(valid_id && valid_library, "invalid OHOS HAP recipe for {id}");
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Target {
    Arm64,
    X64,
}

impl Target {
    pub fn from_rust_target(target: &str) -> Option<Self> {
        [Self::Arm64, Self::X64]
            .into_iter()
            .find(|arch| arch.rust_target() == target)
    }

    pub const fn rust_target(self) -> &'static str {
        match self {
            Self::Arm64 => "aarch64-unknown-linux-ohos",
            Self::X64 => "x86_64-unknown-linux-ohos",
        }
    }

    pub const fn llvm_target(self) -> &'static str {
        match self {
            Self::Arm64 => "aarch64-linux-ohos",
            Self::X64 => "x86_64-linux-ohos",
        }
    }

    pub const fn abi(self) -> &'static str {
        match self {
            Self::Arm64 => "arm64-v8a",
            Self::X64 => "x86_64",
        }
    }

    pub const fn vcpkg_triplet(self) -> &'static str {
        match self {
            Self::Arm64 => "arm64-ohos",
            Self::X64 => "x64-ohos",
        }
    }

    pub const fn architecture(self) -> object::Architecture {
        match self {
            Self::Arm64 => object::Architecture::Aarch64,
            Self::X64 => object::Architecture::X86_64,
        }
    }

    pub const fn local_qa(self) -> bool {
        matches!(self, Self::X64)
    }

    pub const fn page_size(self) -> u64 {
        match self {
            Self::Arm64 | Self::X64 => 4096,
        }
    }

    pub fn cxx_runtime_dir(self, sdk: &Path) -> PathBuf {
        sdk.join("llvm/lib").join(self.llvm_target())
    }
}
