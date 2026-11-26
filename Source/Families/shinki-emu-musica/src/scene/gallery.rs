use super::*;
use crate::{MusicaSystemPage, MusicaVm};
impl Scene {
    pub(crate) fn render_authored_gallery(
        &mut self,
        screen: &crate::runtime::gallery::GalleryScreen,
        cursor: &crate::runtime::gallery::GalleryCursor,
        access: &crate::runtime::gallery::GalleryAccess<'_>,
    ) -> SessionResult<()> {
        use crate::runtime::gallery::GalleryAction;

        let mut commands = vec![SceneCommand::Clear { rgba: [0, 0, 0, 255] }];
        if let Some((button, image)) = cursor.detail
            && let GalleryAction::View { images, .. } = &screen.buttons[button].action
        {
            self.layer(&mut commands, &images[image], 0, 0, 1.0)?;
            return self.submit(commands);
        }
        for layer in &screen.layers {
            self.gallery_layer(&mut commands, layer, [0, 0])?;
        }
        for button in &screen.buttons {
            let art = if button.enabled(access) {
                &button.thumbnail
            } else {
                &button.locked_thumbnail
            };
            if let Some(thumb) = art {
                self.layer(&mut commands, thumb, button.rect[0], button.rect[1], 1.0)?;
            }
        }
        if let Some(button) = cursor.focus.and_then(|i| screen.buttons.get(i))
            && button.enabled(access)
        {
            if let Some(overlay) = button.hover.as_ref().or(screen.hover.as_ref()) {
                let [x, y, width, height] = button.rect;
                commands.push(SceneCommand::PushClip {
                    rect: RectI { x, y, width: width as u32, height: height as u32 },
                });
                self.layer(&mut commands, overlay, 0, 0, 1.0)?;
                commands.push(SceneCommand::PopClip);
            }
            if let Some(preview) = &button.preview {
                self.gallery_layer(&mut commands, preview, [0, 0])?;
            }
        }
        let selected = cursor.selected_track.filter(|(sheet, _)| *sheet == cursor.screen);
        let playing = cursor.playing_track.filter(|(sheet, _)| {
            *sheet == cursor.screen && access.state.audio.get(&0).is_some_and(|stream| stream.playing)
        });
        for (layer, index) in [
            (screen.selection.as_ref(), selected.map(|(_, index)| index)),
            (screen.playing.as_ref(), playing.map(|(_, index)| index)),
        ] {
            if let (Some(layer), Some(index)) = (layer, index) {
                let [x, y, ..] = screen.buttons[index].rect;
                self.gallery_layer(&mut commands, layer, [x, y])?;
            }
        }
        self.submit(commands)
    }

    fn gallery_layer(
        &mut self,
        commands: &mut Vec<SceneCommand>,
        layer: &crate::runtime::gallery::GalleryLayer,
        [x, y]: [i32; 2],
    ) -> SessionResult<()> {
        if let Some([x, y, width, height]) = layer.clip {
            commands.push(SceneCommand::PushClip {
                rect: RectI { x, y, width: width as u32, height: height as u32 },
            });
        }
        self.layer(
            commands,
            &layer.resource,
            x.saturating_add(layer.at[0]),
            y.saturating_add(layer.at[1]),
            1.0,
        )?;
        if layer.clip.is_some() {
            commands.push(SceneCommand::PopClip);
        }
        Ok(())
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
