use super::*;
use serde::{Deserialize, Serialize};

/// Native menus implemented in executable code have no SC layout table. Their
/// release preset supplies this data; script-owned menus run through direct mode.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct FrontendData {
    pub titles: Vec<TitleVariant>,
    pub replays: Vec<GalleryEntry>,
    pub movies: Vec<GalleryEntry>,
    pub cg: Vec<GalleryEntry>,
    pub bgm: Vec<GalleryEntry>,
    pub replay_index: Option<String>,
    pub movie_index: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TitleVariant {
    pub background: String,
    #[serde(default)]
    pub hover: Option<String>,
    #[serde(default)]
    pub requires: BTreeMap<String, i64>,
    pub buttons: Vec<TitleButton>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TitleButton {
    pub rect: [i32; 4],
    pub action: TitleAction,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TitleAction {
    NewGame,
    Load,
    Config,
    Memories,
    Exit,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GalleryEntry {
    pub label: String,
    pub resource: String,
    #[serde(default)]
    pub requires: BTreeMap<String, i64>,
}
fn unlocked(requires: &BTreeMap<String, i64>, state: &MusicaRuntimeState) -> bool {
    requires
        .iter()
        .all(|(key, value)| state.global_variables.get(key).copied().unwrap_or(0) == *value)
}
fn use_discovered(entries: &mut Vec<GalleryEntry>, mut discovered: Vec<GalleryEntry>) {
    if discovered.is_empty() {
        return;
    }
    // Native membership/order wins; metadata supplies semantics the directory
    // cannot encode, including localized labels and unlock requirements.
    let metadata = std::mem::take(entries)
        .into_iter()
        .map(|entry| (entry.resource, (entry.label, entry.requires)))
        .collect::<BTreeMap<_, _>>();
    for entry in &mut discovered {
        if let Some((label, requires)) = metadata.get(&entry.resource) {
            entry.label.clone_from(label);
            entry.requires.clone_from(requires);
        }
    }
    *entries = discovered;
}
impl FrontendData {
    pub(crate) fn load(
        archive: &crate::MusicaMountedVfs,
        json: &str,
        encoding: crate::ScriptEncoding,
        width: u32,
        height: u32,
    ) -> shinki_emu_sdk::runtime::SessionResult<Self> {
        use crate::scene::{core_error, error};
        let mut data: Self = if json.is_empty() {
            Self::default()
        } else {
            serde_json::from_str(json)
                .map_err(|_| error("SHINKI_EMU_MUSICA_FRONTEND", "invalid frontend JSON"))?
        };
        // An explicitly identified original index contains native .chain entries.
        // Do not classify arbitrary story scripts as recollections by their names.
        for (index, entries) in [
            (&data.replay_index, &mut data.replays),
            (&data.movie_index, &mut data.movies),
        ] {
            if let Some(uri) = index {
                let loaded = crate::script_loader::load_script(archive, uri, encoding)?;
                let mut discovered = Vec::new();
                for line in &loaded.script.lines {
                    if let ScLineKind::Command { command } = &line.kind
                        && let ScControlFlow::Chain { target } = &command.control_flow
                    {
                        let resource = format!("musica:/scr/{target}");
                        archive.stat(&resource).map_err(core_error)?;
                        discovered.push(GalleryEntry {
                            label: target.clone(),
                            resource,
                            requires: BTreeMap::new(),
                        });
                    }
                }
                use_discovered(entries, discovered);
            }
        }
        // Physical directory entries reliably identify music, unlike replay purpose.
        let tracks = archive
            .manifest()
            .entries
            .iter()
            .filter(|entry| {
                entry.uri.starts_with("musica:/bgm/")
                    && entry
                        .uri
                        .rsplit_once('.')
                        .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("ogg"))
            })
            .map(|entry| GalleryEntry {
                label: entry.uri.rsplit('/').next().unwrap_or(&entry.uri).into(),
                resource: entry.uri.clone(),
                requires: BTreeMap::new(),
            })
            .collect::<Vec<_>>();
        use_discovered(&mut data.bgm, tracks);
        // Standalone movie wrappers have executable semantics; filenames do not.
        if data.movie_index.is_none() {
            let mut discovered = Vec::new();
            let mut scanned_bytes = 0usize;
            for (index, entry) in archive
                .manifest()
                .entries
                .iter()
                .filter(|e| {
                    e.uri.starts_with("musica:/scr/")
                        && e.uri
                            .rsplit_once('.')
                            .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("sc"))
                })
                .enumerate()
            {
                if index >= 4096 {
                    return Err(error(
                        "SHINKI_EMU_MUSICA_FRONTEND",
                        "movie discovery exceeds the script count bound; supply an original movie_index",
                    ));
                }
                let bytes = crate::script_loader::read_expanded_script(archive, &entry.uri)?;
                scanned_bytes = scanned_bytes
                    .checked_add(bytes.len())
                    .filter(|count| *count <= 64 * 1024 * 1024)
                    .ok_or_else(|| {
                        error(
                            "SHINKI_EMU_MUSICA_FRONTEND",
                            "movie discovery exceeds 64 MiB; supply an original movie_index",
                        )
                    })?;
                if !bytes.windows(6).any(|w| w.eq_ignore_ascii_case(b".movie")) {
                    continue;
                }
                let script = crate::parse_sc_with_encoding(
                    &bytes,
                    &crate::ScOpcodeCatalog::observed_musica(),
                    crate::ScriptEncoding::detect(&bytes, encoding),
                )
                .map_err(|_| {
                    error(
                        "SHINKI_EMU_MUSICA_FRONTEND",
                        "movie script cannot be parsed",
                    )
                })?;
                let mut movie = None;
                let standalone = script.lines.iter().all(|line| match &line.kind {
                    ScLineKind::Blank | ScLineKind::Comment => true,
                    ScLineKind::Command { command } => match command.opcode.as_str() {
                        "movie" if movie.is_none() => {
                            movie = Some(command);
                            true
                        }
                        "end" | "wait" => true,
                        _ => false,
                    },
                    _ => false,
                });
                if standalone && let Some(command) = movie {
                    let label = command
                        .operands
                        .get(1)
                        .and_then(|operand| match operand {
                            ScOperand::Symbol { value } | ScOperand::Text { value } => {
                                Some(value.clone())
                            }
                            _ => None,
                        })
                        .unwrap_or_else(|| entry.uri.clone());
                    match archive.stat(&format!("musica:/mov/{label}")) {
                        Ok(_) => {}
                        Err(cause) if cause.code() == "SHINKI_EMU_VFS_NOT_FOUND" => continue,
                        Err(cause) => return Err(core_error(cause)),
                    }
                    discovered.push(GalleryEntry {
                        label,
                        resource: entry.uri.clone(),
                        requires: BTreeMap::new(),
                    });
                }
            }
            use_discovered(&mut data.movies, discovered);
        }
        if data.titles.len() > 256
            || [&data.replays, &data.movies, &data.cg, &data.bgm]
                .iter()
                .any(|entries| entries.len() > 4096)
        {
            return Err(error(
                "SHINKI_EMU_MUSICA_FRONTEND",
                "frontend count exceeds bounds",
            ));
        }
        for title in &data.titles {
            archive.stat(&title.background).map_err(core_error)?;
            if let Some(hover) = &title.hover {
                archive.stat(hover).map_err(core_error)?;
            }
            if title.buttons.is_empty() || title.buttons.len() > 256 {
                return Err(error(
                    "SHINKI_EMU_MUSICA_FRONTEND",
                    "title must contain menu buttons",
                ));
            }
            for button in &title.buttons {
                let [x, y, w, h] = button.rect;
                if x < 0
                    || y < 0
                    || w <= 0
                    || h <= 0
                    || i64::from(x) + i64::from(w) > i64::from(width)
                    || i64::from(y) + i64::from(h) > i64::from(height)
                {
                    return Err(error(
                        "SHINKI_EMU_MUSICA_FRONTEND",
                        "title hit region is outside the logical canvas",
                    ));
                }
            }
        }
        for entry in data
            .replays
            .iter()
            .chain(&data.movies)
            .chain(&data.cg)
            .chain(&data.bgm)
        {
            if entry.label.is_empty() || entry.label.len() > 512 {
                return Err(error("SHINKI_EMU_MUSICA_FRONTEND", "invalid gallery label"));
            }
            archive.stat(&entry.resource).map_err(core_error)?;
        }
        Ok(data)
    }
}
pub(crate) fn focus_at(title: &TitleVariant, x: f32, y: f32) -> Option<u32> {
    title
        .buttons
        .iter()
        .position(|button| {
            let [left, top, width, height] = button.rect;
            (left as f32..(left + width) as f32).contains(&x)
                && (top as f32..(top + height) as f32).contains(&y)
        })
        .map(|index| index as u32)
}
pub(super) fn return_to_title(state: &mut MusicaRuntimeState) {
    state.wait = None;
    state.message = None;
    state.message_loads.clear();
    state.choice = None;
    state.movie = None;
    state.system_ui = MusicaSystemUiState {
        page: MusicaSystemPage::Title,
        ..Default::default()
    };
}
#[cfg(test)]
pub(super) fn validate(state: &MusicaRuntimeState) -> Result<(), MusicaRuntimeError> {
    if state.system_ui.page == MusicaSystemPage::Title
        && (state.launch_mode != MusicaLaunchMode::Title
            || state.terminal
            || state.wait.is_some()
            || state.system_ui.focus_index >= 256)
    {
        return Err(MusicaRuntimeError::State);
    }
    Ok(())
}
impl MusicaVm {
    pub(crate) fn set_frontend(&mut self, data: FrontendData) {
        self.frontend = std::sync::Arc::new(data);
    }
    pub(crate) fn inherit_frontend(&mut self, source: &Self) {
        self.frontend = source.frontend.clone();
    }
    #[cfg(test)]
    pub(crate) fn frontend_identity(&self) -> *const FrontendData {
        std::sync::Arc::as_ptr(&self.frontend)
    }
    pub(crate) fn title(&self) -> Result<&TitleVariant, MusicaRuntimeError> {
        self.frontend
            .titles
            .iter()
            .rev()
            .find(|title| unlocked(&title.requires, &self.state))
            .ok_or(MusicaRuntimeError::State)
    }
    pub(crate) fn gallery_entries(
        &self,
        page: &MusicaSystemPage,
    ) -> impl Iterator<Item = &GalleryEntry> {
        let entries: &[GalleryEntry] = match page {
            MusicaSystemPage::GalleryReplay => &self.frontend.replays,
            MusicaSystemPage::GalleryMovie => &self.frontend.movies,
            MusicaSystemPage::GalleryCg => &self.frontend.cg,
            MusicaSystemPage::GalleryBgm => &self.frontend.bgm,
            _ => &[],
        };
        entries
            .iter()
            .filter(|entry| unlocked(&entry.requires, &self.state))
    }

    pub(crate) fn start_menu_script(
        &mut self,
        uri: String,
        hash: Hash256,
        script: ScScript,
    ) -> Result<(), MusicaRuntimeError> {
        if !matches!(
            self.state.system_ui.page,
            MusicaSystemPage::Title
                | MusicaSystemPage::GalleryReplay
                | MusicaSystemPage::GalleryMovie
        ) {
            return Err(MusicaRuntimeError::State);
        }
        self.replace_script(uri, hash, script, None)?;
        self.state.effect = None;
        self.state.system_ui.page = MusicaSystemPage::None;
        self.state.system_ui.focus_index = 0;
        Ok(())
    }
    pub fn begin_title_launch(&mut self) -> Result<(), MusicaRuntimeError> {
        if self.state.fixed_tick != 0
            || self.state.pc_line != 0
            || self.state.instruction_count != 0
            || self.state.wait.is_some()
            || self.state.terminal
        {
            return Err(MusicaRuntimeError::State);
        }
        self.title()?;
        self.state.launch_mode = MusicaLaunchMode::Title;
        return_to_title(&mut self.state);
        Ok(())
    }
    pub(crate) fn set_title_focus(&mut self, focus: u32) -> Result<(), MusicaRuntimeError> {
        if self.state.system_ui.page != MusicaSystemPage::Title
            || focus as usize >= self.title()?.buttons.len()
        {
            return Err(MusicaRuntimeError::State);
        }
        self.state.system_ui.focus_index = focus;
        Ok(())
    }
    pub(crate) fn open_title_load(&mut self) -> Result<(), MusicaRuntimeError> {
        if self.state.system_ui.page != MusicaSystemPage::Title {
            return Err(MusicaRuntimeError::State);
        }
        self.state.system_ui.page = MusicaSystemPage::Load;
        self.state.system_ui.focus_index = 0;
        Ok(())
    }
    pub(crate) fn close_title_load(&mut self) -> Result<(), MusicaRuntimeError> {
        if self.state.system_ui.page != MusicaSystemPage::Load {
            return Err(MusicaRuntimeError::State);
        }
        return_to_title(&mut self.state);
        Ok(())
    }
    pub(crate) fn set_launch_mode(&mut self, mode: MusicaLaunchMode) {
        self.state.launch_mode = mode;
    }
}

#[cfg(test)]
pub(crate) fn test_frontend() -> FrontendData {
    serde_json::from_value(serde_json::json!({
        "titles":[
            {"background":"musica:/sys/base.png","buttons":[
                {"rect":[10,20,100,30],"action":"new_game"},
                {"rect":[10,60,100,30],"action":"exit"}
            ]},
            {"background":"musica:/sys/unlocked.png","requires":{"chapter_done":3},"buttons":[
                {"rect":[200,100,80,40],"action":"memories"},
                {"rect":[200,150,80,40],"action":"exit"}
            ]}
        ],
        "bgm":[{"label":"Track","resource":"musica:/bgm/music.ogg"}],
        "replays":[{"label":"Memory","resource":"musica:/scr/memory.sc","requires":{"chapter_done":3}}]
    })).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn machine() -> MusicaVm {
        let source = b".end\r\n";
        let mut vm = MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            crate::parse_sc(source, &crate::ScOpcodeCatalog::observed_musica()).unwrap(),
            0,
        )
        .unwrap();
        vm.set_frontend(test_frontend());
        vm
    }
    #[test]
    fn title_unlocks_actions_and_hit_regions_follow_input_data() {
        let mut vm = machine();
        vm.begin_title_launch().unwrap();
        assert_eq!(focus_at(vm.title().unwrap(), 20.0, 25.0), Some(0));
        assert_eq!(focus_at(vm.title().unwrap(), 220.0, 125.0), None);
        assert_eq!(vm.title().unwrap().background, "musica:/sys/base.png");
        vm.replace_global_variables(BTreeMap::from([("chapter_done".into(), 3)]));
        assert_eq!(vm.title().unwrap().background, "musica:/sys/unlocked.png");
        assert_eq!(focus_at(vm.title().unwrap(), 220.0, 125.0), Some(0));
        assert_eq!(focus_at(vm.title().unwrap(), f32::NAN, 125.0), None);
        assert!(vm.set_title_focus(2).is_err());
    }
    #[test]
    fn gallery_focus_wrap_audio_and_unlocks_use_actual_entries() {
        let mut vm = machine();
        assert!(vm.set_gallery_page(MusicaSystemPage::Memories, 0).is_err());
        vm.begin_title_launch().unwrap();
        assert!(
            vm.set_gallery_page(MusicaSystemPage::GalleryReplay, 0)
                .is_err()
        );
        vm.replace_global_variables(BTreeMap::from([("chapter_done".into(), 3)]));
        vm.set_gallery_page(MusicaSystemPage::GalleryReplay, 0)
            .unwrap();
        vm.move_gallery_focus(-1).unwrap();
        assert_eq!(vm.state.system_ui.focus_index, 0);
        let before = vm.state.clone();
        assert!(
            vm.set_gallery_page(MusicaSystemPage::GalleryReplay, 1)
                .is_err()
        );
        assert_eq!(vm.state, before);
        vm.set_gallery_page(MusicaSystemPage::GalleryBgm, 0)
            .unwrap();
        assert_eq!(vm.gallery_bgm_play().unwrap().len(), 2);
        assert_eq!(vm.state.audio[&0].resource_uri, "musica:/bgm/music.ogg");
        assert_eq!(vm.gallery_bgm_play().unwrap().len(), 3);
        assert_eq!(vm.gallery_bgm_stop().unwrap().len(), 1);
        assert!(vm.gallery_bgm_stop().unwrap().is_empty());
        vm.set_gallery_page(MusicaSystemPage::Title, 0).unwrap();
        assert!(vm.gallery_bgm_play().is_err());
    }
    #[test]
    fn native_indexes_and_standalone_movies_discover_entries_without_name_patterns() {
        let root = tempfile::tempdir().unwrap();
        crate::test_fixture::game(root.path(), b".end\r\n");
        crate::test_fixture::assets(
            root.path(),
            "scr",
            &[
                ("test.sc", b".end\r\n"),
                ("index.sc", b".chain recollection.sc\r\n.end\r\n"),
                ("recollection.sc", b".message 1   Memory\r\n.end\r\n"),
                (
                    "standalone.sc",
                    b".movie 1 authored.avi 640 360 t\r\n.end\r\n",
                ),
                (
                    "story.sc",
                    b".message 1   Story\r\n.movie 1 authored.avi 640 360 t\r\n.end\r\n",
                ),
            ],
        );
        // Discovery reads the PAZ directory, not the movie payload or decoder.
        crate::test_fixture::asset(root.path(), "mov", "authored.avi", b"directory-fixture");
        let archive = crate::mount_musica(root.path(), std::path::Path::new("key.toml")).unwrap();
        let data = FrontendData::load(
            &archive,
            r#"{"replay_index":"musica:/scr/index.sc"}"#,
            crate::ScriptEncoding::ShiftJis,
            640,
            360,
        )
        .unwrap();
        assert_eq!(data.replays.len(), 1);
        assert_eq!(data.replays[0].resource, "musica:/scr/recollection.sc");
        assert_eq!(data.movies.len(), 1);
        assert_eq!(data.movies[0].resource, "musica:/scr/standalone.sc");
        assert_eq!(data.movies[0].label, "authored.avi");
        assert!(FrontendData::load(&archive, r#"{"titles":[{"background":"musica:/mov/authored.avi","buttons":[{"rect":[600,0,100,40],"action":"exit"}]}]}"#, crate::ScriptEncoding::ShiftJis, 640, 360).is_err());
    }
}
