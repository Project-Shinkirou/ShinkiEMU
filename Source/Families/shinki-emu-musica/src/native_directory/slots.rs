use super::*;
use crate::{
    eden_save::{EdenEdition, EdenSave, EdenSaveEncoding},
    storage::{SAVE_MAX_SLOTS, SaveCard},
};

impl NativeDirectory {
    fn slot_stem(&self, slot: u32, append_playing: bool) -> SessionResult<String> {
        if slot >= SAVE_MAX_SLOTS {
            return Err(error(
                "SHINKI_EMU_MUSICA_SAVE_SLOT",
                "native save slot is out of range",
            ));
        }
        Ok(match self.title {
            MinoriTitle::EdenEnglish if append_playing => format!("edenPM{slot:04}"),
            MinoriTitle::EdenEnglish => format!("eden{slot:04}"),
            MinoriTitle::Perseus => format!("perseus_{slot:04}"),
        })
    }
    fn slot_save(
        &self,
        slot: u32,
        append_playing: bool,
        encoding: ScriptEncoding,
    ) -> SessionResult<Option<EdenSave>> {
        let stem = self.slot_stem(slot, append_playing)?;
        let Some(bytes) = read_optional(&self.root.join(format!("{stem}.sav")))? else {
            return Ok(None);
        };
        let edition = EdenEdition::for_title(self.title);
        let encoding = EdenSaveEncoding::from_script(encoding);
        let save = EdenSave::decode(&bytes, edition, encoding).map_err(core_error)?;
        if self.title == MinoriTitle::Perseus
            && self.prefix.strict
            && save.prefix != self.prefix.value
        {
            return Err(error(
                "SHINKI_EMU_MUSICA_SAVE_PREFIX",
                "slot serial code does not match this game",
            ));
        }
        Ok(Some(save))
    }
}

impl NativeFiles {
    pub(crate) fn edition(&self) -> Option<EdenEdition> {
        self.dir
            .as_ref()
            .map(|dir| EdenEdition::for_title(dir.title))
    }
    fn slot_dir(&self) -> SessionResult<&NativeDirectory> {
        self.dir.as_ref().ok_or_else(|| {
            error(
                "SHINKI_EMU_MUSICA_SAVE_TITLE",
                "native slots require an identified original game executable",
            )
        })
    }
    pub(crate) fn slot(&self, slot: u32) -> SessionResult<Option<EdenSave>> {
        let dir = self.slot_dir()?;
        let append = self
            .globals()
            .get("AppendPlaying")
            .is_some_and(|value| *value != 0);
        dir.slot_save(slot, append, self.encoding)
    }
    pub(crate) fn card(&self, slot: u32) -> SessionResult<Option<SaveCard>> {
        let dir = self.slot_dir()?;
        let append = self
            .globals()
            .get("AppendPlaying")
            .is_some_and(|value| *value != 0);
        let Some(save) = dir.slot_save(slot, append, self.encoding)? else {
            return Ok(None);
        };
        let stem = dir.slot_stem(slot, append)?;
        let png = read_optional(&dir.root.join(format!("{stem}.png")))?.ok_or_else(|| {
            error(
                "SHINKI_EMU_MUSICA_SAVE_THUMBNAIL",
                "native slot thumbnail is missing",
            )
        })?;
        let mtime = std::fs::metadata(dir.root.join(format!("{stem}.sav")))
            .and_then(|meta| meta.modified())
            .map_err(|_| {
                error(
                    "SHINKI_EMU_MUSICA_SAVE_READ",
                    "save modification time is unavailable",
                )
            })?;
        SaveCard::from_native(save.edition, save.comment, png, mtime).map(Some)
    }
    pub(crate) fn write_slot(
        &self,
        slot: u32,
        save: &EdenSave,
        thumbnail_png: &[u8],
    ) -> SessionResult<()> {
        let dir = self.slot_dir()?;
        if save.edition != EdenEdition::for_title(dir.title)
            || (dir.title == MinoriTitle::Perseus && save.prefix != dir.prefix.value)
        {
            return Err(error(
                "SHINKI_EMU_MUSICA_SAVE_GAME",
                "slot edition or serial code differs from this game",
            ));
        }
        let stem = dir.slot_stem(
            slot,
            self.globals().get("AppendPlaying").is_some_and(|v| *v != 0),
        )?;
        // Do not replace a malformed existing native slot.
        dir.slot_save(
            slot,
            self.globals().get("AppendPlaying").is_some_and(|v| *v != 0),
            self.encoding,
        )?;
        let bytes = save.encode().map_err(core_error)?;
        // Native writer persists the bitmap first, then the compressed slot.
        replace_file(&dir.root, &format!("{stem}.png"), thumbnail_png)?;
        replace_file(&dir.root, &format!("{stem}.sav"), &bytes)
    }
    pub(crate) fn save_edition(&self) -> SessionResult<(EdenEdition, EdenSaveEncoding, u32)> {
        let dir = self.slot_dir()?;
        Ok((
            EdenEdition::for_title(dir.title),
            EdenSaveEncoding::from_script(self.encoding),
            dir.prefix.value,
        ))
    }
    pub(crate) fn is_perseus(&self) -> SessionResult<bool> {
        Ok(self.slot_dir()?.title == MinoriTitle::Perseus)
    }
}
