use super::*;
use crate::{MusicaSystemPage, MusicaVm};
impl Scene {
    pub fn render_gallery(&mut self, vm: &MusicaVm) -> SessionResult<()> {
        let page = &vm.state().system_ui.page;
        let focus = vm.state().system_ui.focus_index;
        if vm.gallery_count(page).is_none_or(|count| focus >= count) {
            return Err(error(
                "SHINKI_EMU_MUSICA_GALLERY_STATE",
                "gallery page or focus is invalid",
            ));
        }
        let mut commands = vec![SceneCommand::Clear {
            rgba: [0, 0, 0, 255],
        }];
        if *page == MusicaSystemPage::GalleryCg {
            let entry = vm
                .gallery_entries(page)
                .nth(focus as usize)
                .ok_or_else(|| error("SHINKI_EMU_MUSICA_GALLERY_STATE", "CG entry is absent"))?;
            self.layer(&mut commands, &entry.resource, 0, 0, 1.0)?;
        }
        let text = if *page == MusicaSystemPage::Memories {
            self.text.gallery_list(
                vm.gallery_pages().map(|page| match page {
                    MusicaSystemPage::GalleryCg => "CG",
                    MusicaSystemPage::GalleryReplay => "Replay",
                    MusicaSystemPage::GalleryBgm => "BGM",
                    MusicaSystemPage::GalleryMovie => "Movie",
                    _ => "Title",
                }),
                focus,
                self.width,
                self.height,
            )
        } else {
            self.text.gallery_list(
                vm.gallery_entries(page).map(|entry| entry.label.as_str()),
                focus,
                self.width,
                self.height,
            )
        }
        .map_err(|code| error(&code, "gallery labels could not be rendered"))?;
        self.append_text_commands(&mut commands, text);
        self.submit(commands)
    }
}
