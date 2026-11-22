//! The same drive can hold different games at the same relative path. Use an
//! opaque library location and keep the validated native path in app state.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct OpticalGame {
    pub(super) volume: OpticalVolume,
    pub(super) path: PathBuf,
}

pub(super) fn is_optical(location: &str) -> bool {
    location.starts_with("optical:")
}

pub(super) fn game_location(volume: &OpticalVolume, path: &Path) -> Result<String, String> {
    let path = path.to_str().ok_or("SHINKI_EMU_GAME_PATH_UTF8")?;
    let identity = volume.identity();
    Ok(format!("optical:{}", sha256_id(&format!("{}:{identity}{path}", identity.len()))))
}

impl ShinkiEmuManagerController {
    pub(super) fn bind_scanned_optical_game(
        &mut self,
        mut game: GameRecord,
        volume: &OpticalVolume,
        path: &Path,
    ) -> Result<GameRecord, String> {
        game.location = game_location(volume, path)?;
        game.game_id = format!("game-{}", &game.location["optical:".len()..]);
        if dunce::simplified(path) == volume.root
            && let Some(label) = volume.label.as_ref().filter(|label| !label.trim().is_empty())
        {
            game.title.clone_from(label);
        }
        self.remember_optical_game(&game, volume, path)?;
        Ok(game)
    }

    pub(super) fn scan_optical_game_path<'a>(&'a self, location: &str, volume: &OpticalVolume) -> Option<&'a str> {
        let binding = self.optical_games.get(location)?;
        if binding.volume.root != volume.root || binding.volume.identity() != volume.identity() {
            return None;
        }
        binding.path.to_str()
    }

    fn remember_optical_game(&mut self, game: &GameRecord, volume: &OpticalVolume, path: &Path) -> Result<(), String> {
        if self.optical_games.contains_key(&game.location) {
            return Ok(());
        }
        let binding = OpticalGame { volume: volume.clone(), path: path.to_owned() };
        self.library.set_app_state(&game.location, &binding).map_err(|error| error.to_string())?;
        self.optical_games.insert(game.location.clone(), binding);
        Ok(())
    }

    pub(super) fn available_optical_game(&self, game: &GameRecord) -> Option<&OpticalGame> {
        self.optical_games.get(&game.location).filter(|binding| {
            self.opened_optical.get(&binding.volume.root)
                .is_some_and(|current| current.identity() == binding.volume.identity())
        })
    }

    pub(super) fn validate_optical_launch(&mut self, game: &GameRecord) -> Result<Option<OpticalGame>, String> {
        if !is_optical(&game.location) {
            return Ok(None);
        }
        let binding = self.available_optical_game(game).cloned().ok_or(OPTICAL_MEDIA_UNAVAILABLE)?;
        if let Err(error) = binding.volume.validate() {
            self.opened_optical.remove(&binding.volume.root);
            return Err(error);
        }
        Ok(Some(binding))
    }
}

#[cfg(test)]
mod tests {
    use super::super::recognition_tests::{import, open, settle, write_game};
    use super::*;

    fn selection(root: &Path, identity: &str) -> OpticalVolume {
        serde_json::from_value(serde_json::json!({
            "root": root,
            "label": "Test disc",
            "identity": identity,
        })).unwrap()
    }

    fn register(app: &mut ShinkiEmuManagerController, volume: &OpticalVolume, path: &Path) -> GameRecord {
        let game = GameRecord {
            game_id: path_id(path),
            title: title_for_path(path),
            user_title: None,
            location: path.to_str().unwrap().into(),
            family_id: None,
            content_fingerprint: None,
            added_at_unix_ms: 1,
        };
        let game = app.bind_scanned_optical_game(game, volume, path).unwrap();
        app.library.add_game(&game).unwrap();
        game
    }

    #[test]
    fn media_replacement_at_same_root_preserves_both_records() {
        let data = tempfile::tempdir().unwrap();
        let mount = tempfile::tempdir().unwrap();
        let path = mount.path().join("game");
        write_game(&path);
        let first = selection(mount.path(), "first");
        let second = selection(mount.path(), "second");
        let mut app = open(data.path());
        let game_a = register(&mut app, &first, &path);
        let game_b = register(&mut app, &second, &path);
        assert_ne!(game_a.game_id, game_b.game_id);
        assert!(app.scan_optical_game_path(&game_a.location, &second).is_none());
        assert_eq!(app.scan_optical_game_path(&game_b.location, &second), path.to_str());
        app.opened_optical.insert(first.root.clone(), first.clone());
        assert!(app.available_optical_game(&game_a).is_some());
        app.opened_optical.insert(second.root.clone(), second);
        assert!(app.available_optical_game(&game_a).is_none());
        assert!(app.available_optical_game(&game_b).is_some());
        assert_eq!(register(&mut app, &first, &path).game_id, game_a.game_id);
        app.opened_optical.insert(first.root.clone(), first);
        assert!(app.available_optical_game(&game_a).is_some());
        assert!(app.available_optical_game(&game_b).is_none());
        drop(app);

        let reopened = open(data.path());
        assert_eq!(reopened.library.list_games().unwrap().len(), 2);
        assert!(reopened.available_optical_game(&game_a).is_none());
        assert_eq!(reopened.display_game_location(&game_a.location), path.to_str().unwrap());
        assert!(reopened.scan_roots().unwrap().is_empty());
    }

    #[test]
    fn launching_an_ejected_optical_game_does_not_follow_the_underlying_directory() {
        let data = tempfile::tempdir().unwrap();
        let mount = tempfile::tempdir().unwrap();
        let path = mount.path().join("game");
        write_game(&path);
        let volume = selection(mount.path(), "previously mounted");
        let mut app = open(data.path());
        let game = register(&mut app, &volume, &path);
        app.opened_optical.insert(volume.root.clone(), volume);
        app.set_host_wake(std::sync::Arc::new(|| {}));
        settle(&mut app);
        assert_eq!(app.launch(&game.game_id).err().as_deref(), Some(OPTICAL_MEDIA_UNAVAILABLE));
        assert!(app.available_optical_game(&game).is_none());
        assert!(app.active.is_none());
    }

    #[test]
    fn stale_optical_import_registers_nothing_and_leaves_ordinary_import_usable() {
        let data = tempfile::tempdir().unwrap();
        let mount = tempfile::tempdir().unwrap();
        write_game(mount.path());
        let mut app = open(data.path());
        app.set_host_wake(std::sync::Arc::new(|| {}));
        settle(&mut app);
        app.scan_optical_volume(selection(mount.path(), "previously mounted")).unwrap();
        settle(&mut app);
        assert_eq!(app.diagnostic, OPTICAL_MEDIA_UNAVAILABLE);
        assert!(app.library.list_games().unwrap().is_empty());
        assert!(app.scan_roots().unwrap().is_empty());
        let game = import(&mut app, mount.path());
        assert!(!is_optical(&app.game(&game).unwrap().location));
    }
}
