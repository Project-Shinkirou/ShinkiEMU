use super::*;
use crate::{MusicaSystemPage, MusicaVm};
impl Scene {
    pub(crate) fn render_authored_gallery(
        &mut self,
        screen: &crate::runtime::gallery::GalleryScreen,
        cursor: &crate::runtime::gallery::GalleryCursor,
        state: &crate::MusicaRuntimeState,
    ) -> SessionResult<()> {
        use crate::runtime::gallery::GalleryAction;

        let mut commands = vec![SceneCommand::Clear { rgba: [0, 0, 0, 255] }];
        if let Some((button, image)) = cursor.detail
            && let GalleryAction::View { images } = &screen.buttons[button].action
        {
            self.layer(&mut commands, &images[image], 0, 0, 1.0)?;
            return self.submit(commands);
        }
        for layer in &screen.layers {
            self.layer(&mut commands, &layer.resource, layer.at[0], layer.at[1], 1.0)?;
        }
        for button in &screen.buttons {
            if let Some(thumb) = &button.thumbnail
                && button.enabled(state)
            {
                self.layer(&mut commands, thumb, button.rect[0], button.rect[1], 1.0)?;
            }
        }
        if let Some(button) = cursor.focus.and_then(|i| screen.buttons.get(i))
            && let Some(overlay) = button.hover.as_ref().or(screen.hover.as_ref())
            && button.enabled(state)
        {
            let [x, y, width, height] = button.rect;
            commands.push(SceneCommand::PushClip {
                rect: RectI { x, y, width: width as u32, height: height as u32 },
            });
            self.layer(&mut commands, overlay, 0, 0, 1.0)?;
            commands.push(SceneCommand::PopClip);
        }
        let selected = cursor.selected_track.filter(|(sheet, _)| *sheet == cursor.screen);
        let playing = cursor.playing_track.filter(|(sheet, _)| {
            *sheet == cursor.screen && state.audio.get(&0).is_some_and(|stream| stream.playing)
        });
        for (layer, index) in [
            (screen.selection.as_ref(), selected.map(|(_, index)| index)),
            (screen.playing.as_ref(), playing.map(|(_, index)| index)),
        ] {
            if let (Some(layer), Some(index)) = (layer, index) {
                let [x, y, ..] = screen.buttons[index].rect;
                self.layer(&mut commands, &layer.resource, x.saturating_add(layer.at[0]), y.saturating_add(layer.at[1]), 1.0)?;
            }
        }
        self.submit(commands)
    }

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
