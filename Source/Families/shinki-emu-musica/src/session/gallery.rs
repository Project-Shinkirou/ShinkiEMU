use super::*;
use crate::{MusicaSystemPage, runtime::gallery};
impl MusicaSession {
    fn activate_gallery(&mut self) -> SessionResult<()> {
        let focus = self.vm.state().system_ui.focus_index;
        match self.vm.state().system_ui.page {
            MusicaSystemPage::Memories => {
                let page = self.vm.gallery_pages().nth(focus as usize).ok_or_else(|| {
                    error(
                        "SHINKI_EMU_MUSICA_GALLERY_FOCUS",
                        "invalid gallery category",
                    )
                })?;
                self.vm.set_gallery_page(page, 0).map_err(vm_error)
            }
            MusicaSystemPage::GalleryBgm => {
                let commands = self.vm.gallery_bgm_play().map_err(vm_error)?;
                self.audio.apply(commands)
            }
            MusicaSystemPage::GalleryReplay | MusicaSystemPage::GalleryMovie => {
                let target = self
                    .vm
                    .gallery_entries(&self.vm.state().system_ui.page)
                    .nth(focus as usize)
                    .ok_or_else(|| {
                        error(
                            "SHINKI_EMU_MUSICA_GALLERY_FOCUS",
                            "invalid script gallery focus",
                        )
                    })?
                    .resource
                    .clone();
                self.start_menu_script(target)
            }
            MusicaSystemPage::GalleryCg => Ok(()),
            _ => Err(error(
                "SHINKI_EMU_MUSICA_GALLERY_FOCUS",
                "invalid gallery page",
            )),
        }
    }
    pub(super) fn gallery_event(&mut self, event: &RuntimeEvent) -> SessionResult<bool> {
        let page = self.vm.state().system_ui.page.clone();
        if !gallery::is_gallery(&page) {
            return Ok(false);
        }
        match event {
            RuntimeEvent::Key {
                code: KeyCode::Escape,
                state: KeyState::Pressed,
                ..
            } => {
                if page == MusicaSystemPage::GalleryBgm {
                    let commands = self.vm.gallery_bgm_stop().map_err(vm_error)?;
                    self.audio.apply(commands)?;
                }
                self.vm
                    .set_gallery_page(
                        if page == MusicaSystemPage::Memories {
                            MusicaSystemPage::Title
                        } else {
                            MusicaSystemPage::Memories
                        },
                        0,
                    )
                    .map_err(vm_error)?;
                self.title_focus = None;
            }
            RuntimeEvent::Key {
                code: KeyCode::ArrowUp | KeyCode::ArrowLeft,
                state: KeyState::Pressed,
                ..
            } => self.vm.move_gallery_focus(-1).map_err(vm_error)?,
            RuntimeEvent::Key {
                code: KeyCode::ArrowDown | KeyCode::ArrowRight,
                state: KeyState::Pressed,
                ..
            } => self.vm.move_gallery_focus(1).map_err(vm_error)?,
            RuntimeEvent::Key {
                code: KeyCode::Enter | KeyCode::Space,
                state: KeyState::Pressed,
                ..
            } => {
                self.activate_gallery()?;
            }
            RuntimeEvent::Key {
                code: KeyCode::Backspace,
                state: KeyState::Pressed,
                ..
            } if page == MusicaSystemPage::GalleryBgm => {
                let commands = self.vm.gallery_bgm_stop().map_err(vm_error)?;
                self.audio.apply(commands)?;
            }
            RuntimeEvent::PointerMove { x, y } => self.pointer = Some((*x, *y)),
            RuntimeEvent::PointerButton {
                button: PointerButton::Primary,
                state: KeyState::Pressed,
            } => {
                if let Some((x, y)) = self.pointer {
                    let count = self.vm.gallery_count(&page).unwrap_or(0);
                    let width = self.info.logical_width as f32;
                    let height = self.info.logical_height as f32;
                    if (width * 0.05..width * 0.95).contains(&x)
                        && (height * 0.1..height * 0.9).contains(&y)
                    {
                        let focus = self.vm.state().system_ui.focus_index / 12 * 12
                            + ((y / height - 0.1) / (0.8 / 12.0)) as u32;
                        if focus < count {
                            self.vm.set_gallery_page(page, focus).map_err(vm_error)?;
                            self.activate_gallery()?;
                        }
                    }
                }
            }
            RuntimeEvent::WindowFocused { .. }
            | RuntimeEvent::WindowResized { .. }
            | RuntimeEvent::WindowVisibility { .. }
            | RuntimeEvent::WindowSuspended { .. } => return Ok(false),
            _ => {}
        }
        Ok(true)
    }
}
