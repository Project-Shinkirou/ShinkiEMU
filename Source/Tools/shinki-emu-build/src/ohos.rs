//! Architecture mapping shared by the OHOS build scripts and HAP packager.
use std::path::{Path, PathBuf};

pub const MANAGER_LIBRARY: &str = "libshinki_emu_manager.so";

/// A first-party DSO bundled only on the selected OHOS ABIs.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HapCore {
    library: String,
    #[serde(default = "all_abis")]
    abis: Vec<Target>,
}

fn all_abis() -> Vec<Target> {
    Target::ALL.to_vec()
}

impl HapCore {
    pub fn library_for(&self, target: Target) -> Option<&str> {
        self.abis.contains(&target).then_some(&self.library)
    }

    pub fn validate(&self, id: &str) -> anyhow::Result<()> {
        let valid_id = !matches!(id, "" | "." | ".." | "manager")
            && id.len() <= 128
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
        let valid_library = !self.library.eq_ignore_ascii_case(MANAGER_LIBRARY)
            && self
                .library
                .strip_prefix("libshinki_emu_")
                .and_then(|name| name.strip_suffix(".so"))
                .is_some_and(|name| {
                    !name.is_empty()
                        && name
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                });
        anyhow::ensure!(
            valid_id
                && valid_library
                && matches!(self.abis.as_slice(), [_] | [Target::Arm64, Target::X64] | [Target::X64, Target::Arm64]),
            "invalid OHOS HAP recipe for {id}"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize)]
pub enum Target {
    #[serde(rename = "arm64-v8a")]
    Arm64,
    #[serde(rename = "x86_64")]
    X64,
}

impl Target {
    pub const ALL: [Self; 2] = [Self::Arm64, Self::X64];

    pub fn from_rust_target(target: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|arch| arch.rust_target() == target)
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
