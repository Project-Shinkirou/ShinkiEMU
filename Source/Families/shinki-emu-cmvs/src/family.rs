use crate::{CMVS_PROFILE_FILE, mount_cmvs, session::CmvsSession};
use shinki_emu_sdk::runtime::*;
use std::{
    io::Read,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
pub(crate) fn error(code: &str, message: &str) -> SessionError {
    SessionError::invalid(code, message)
}
pub(crate) fn core(error: shinki_emu_sdk::CoreError) -> SessionError {
    SessionError::invalid(error.code(), error.message())
}
#[cfg(test)]
pub(crate) static TEST_SESSION_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
static ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) struct Lease;
impl Lease {
    pub(crate) fn acquire() -> SessionResult<Self> {
        ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                error(
                    "SHINKI_EMU_CMVS_SESSION_ACTIVE",
                    "one CMVS session may be active",
                )
            })?;
        Ok(Self)
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}
#[derive(Default)]
pub(crate) struct CmvsProvider {
    gpu_instance: Option<wgpu::Instance>,
}
pub(crate) fn cmvs_descriptor() -> RuntimeDescriptor {
    let field = |id: &str, label: &str, default: &str| ConfigField {
        id: id.into(),
        label: label.into(),
        group: "Files".into(),
        kind: ConfigKind::String { max_bytes: 256 },
        default: ConfigValue::String(default.into()),
    };
    RuntimeDescriptor {
        family_id: "cmvs".into(),
        plugin_id: "shinki.emu.cmvs".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![RuntimeCapability::CpuFrame, RuntimeCapability::PcmAudio],
        supported_formats: vec!["cmvs.cpz5".into()],
        configuration: vec![
            field(
                "profile_file",
                "Private CPZ mount profile",
                CMVS_PROFILE_FILE,
            ),
            field("entry_script", "Entry PS2A script", "start.ps3"),
        ],
    }
}
impl CmvsProvider {
    pub(crate) fn descriptor(&self) -> SessionResult<RuntimeDescriptor> {
        let d = cmvs_descriptor();
        d.validate()?;
        Ok(d)
    }
    pub(crate) fn probe(&self, request: ProbeRequest) -> SessionResult<Option<ProbeReport>> {
        request.validate()?;
        let root = Path::new(request.game_path.as_str());
        if !root.is_dir() {
            return Ok(None);
        }
        let found = [root.to_path_buf(), root.join("data/pack")]
            .into_iter()
            .filter_map(|directory| std::fs::read_dir(directory).ok())
            .flatten()
            .take(4096)
            .filter_map(Result::ok)
            .any(|entry| {
                let path = entry.path();
                if !path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("cpz"))
                {
                    return false;
                }
                let mut magic = [0; 4];
                std::fs::File::open(path)
                    .and_then(|mut f| f.read_exact(&mut magic))
                    .is_ok()
                    && &magic == b"CPZ5"
            });
        if !found {
            return Ok(None);
        }
        let game_id = root
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| error("SHINKI_EMU_CMVS_GAME_ID", "game directory name is invalid"))?;
        Ok(Some(ProbeReport {
            family_id: "cmvs".into(),
            game_id: game_id.into(),
            format: "cmvs.cpz5".into(),
            confidence_permyriad: 9500,
        }))
    }
    pub(crate) fn open(
        &mut self,
        game_path: &str,
        configuration: &[ConfigEntry],
        window: WindowState,
    ) -> SessionResult<CmvsSession> {
        let d = self.descriptor()?;
        validate_game_path(game_path)?;
        window.validate()?;
        let values = resolve_config(&d.configuration, configuration)?;
        let string = |index: usize| match &values[index].value {
            ConfigValue::String(v) => Ok(v.as_str()),
            _ => Err(error("SHINKI_EMU_CMVS_CONFIG", "expected filename")),
        };
        let profile = string(0)?;
        let entry = string(1)?;
        let lease = Lease::acquire()?;
        let game_root = Path::new(game_path).to_path_buf();
        let archive = Arc::new(mount_cmvs(&game_root, Path::new(profile)).map_err(core)?);
        if self.gpu_instance.is_none() {
            self.gpu_instance = Some(roukaku_platform_common::native_wgpu_instance().map_err(
                |_| {
                    error(
                        "SHINKI_EMU_CMVS_GPU_CREATE",
                        "native GPU instance creation failed",
                    )
                },
            )?);
        }
        let session = CmvsSession::new(
            archive,
            game_root.clone(),
            entry,
            window,
            lease,
            self.gpu_instance
                .as_ref()
                .expect("GPU instance initialized"),
        )?;
        Ok(session)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    pub(crate) fn probe_uses_native_magic_without_game_specific_names() {
        let root = tempfile::tempdir().unwrap();
        let p = CmvsProvider::default();
        let request = || ProbeRequest {
            game_path: root.path().to_string_lossy().to_string(),
        };
        assert!(p.probe(request()).unwrap().is_none());
        std::fs::write(root.path().join("any.cpz"), b"CPZ6").unwrap();
        assert!(p.probe(request()).unwrap().is_none());
        std::fs::write(root.path().join("any.cpz"), b"CPZ5").unwrap();
        assert!(p.probe(request()).unwrap().is_some());
        std::fs::remove_file(root.path().join("any.cpz")).unwrap();
        std::fs::create_dir_all(root.path().join("data/pack")).unwrap();
        std::fs::write(root.path().join("data/pack/any.cpz"), b"CPZ5").unwrap();
        assert!(p.probe(request()).unwrap().is_some());
    }
    #[test]
    fn missing_private_profile_releases_lease_and_preserves_files() {
        let _session = TEST_SESSION_LOCK.lock();
        let root = tempfile::tempdir().unwrap();
        let mut p = CmvsProvider::default();
        for _ in 0..2 {
            let result = p.open(
                root.path().to_str().unwrap(),
                &[],
                WindowState {
                    width: 640,
                    height: 480,
                    focused: true,
                    visible: true,
                },
            );
            assert!(result.is_err());
            assert!(!ACTIVE.load(Ordering::Acquire));
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        }
    }
}
