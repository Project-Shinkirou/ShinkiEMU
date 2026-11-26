use super::*;
use crate::runtime::gallery::{GalleryAction, GalleryCursor, GalleryDirection};

impl MusicaSession {
    pub(in crate::session) fn open_memories(&mut self) -> SessionResult<()> {
        self.vm.set_gallery_page(MusicaSystemPage::Memories, 0).map_err(vm_error)?;
        self.gallery_cursor = self.vm.authored_gallery().map(|layout| {
            GalleryCursor::new(layout, self.vm.state())
        });
        Ok(())
    }

    pub(super) fn authored_gallery_event(&mut self, event: &RuntimeEvent) -> SessionResult<bool> {
        let cursor = self.gallery_cursor.as_mut().expect("active gallery cursor");
        let layout = self.vm.authored_gallery().expect("active gallery layout");
        let screen = &layout.screens[cursor.screen];
        let state = self.vm.state();
        let action = match event {
            RuntimeEvent::WindowFocused { focused: false } => {
                cursor.pressed = None;
                cursor.focus = None;
                return Ok(false);
            }
            RuntimeEvent::WindowFocused { .. }
            | RuntimeEvent::WindowResized { .. }
            | RuntimeEvent::WindowVisibility { .. }
            | RuntimeEvent::WindowSuspended { .. } => return Ok(false),
            RuntimeEvent::PointerMove { x, y } => {
                self.pointer = Some((*x, *y));
                if cursor.detail.is_none() {
                    cursor.focus = screen.hit(*x, *y, state);
                }
                None
            }
            RuntimeEvent::PointerButton { button: PointerButton::Primary, state: KeyState::Pressed } => {
                cursor.pressed = if cursor.detail.is_some() {
                    None
                } else {
                    self.pointer.and_then(|(x, y)| screen.hit(x, y, state))
                };
                None
            }
            RuntimeEvent::PointerButton { button: PointerButton::Primary, state: KeyState::Released } => {
                if cursor.detail.is_some() {
                    self.step_gallery_image(true);
                    return Ok(true);
                }
                cursor.focus = self.pointer.and_then(|(x, y)| screen.hit(x, y, state));
                cursor.pressed.take().filter(|&i| Some(i) == cursor.focus)
            }
            RuntimeEvent::Key { code, state: KeyState::Pressed, .. } => match code {
                KeyCode::Escape => {
                    self.authored_gallery_back()?;
                    return Ok(true);
                }
                KeyCode::Enter | KeyCode::Space | KeyCode::ArrowRight if cursor.detail.is_some() => {
                    self.step_gallery_image(true);
                    return Ok(true);
                }
                KeyCode::ArrowLeft if cursor.detail.is_some() => {
                    self.step_gallery_image(false);
                    return Ok(true);
                }
                _ if cursor.detail.is_some() => None,
                KeyCode::Enter | KeyCode::Space if screen.track_group.is_some() => {
                    self.play_selected_track()?;
                    return Ok(true);
                }
                KeyCode::Enter | KeyCode::Space => cursor.focus,
                KeyCode::PageUp | KeyCode::PageDown => {
                    layout.page_button(cursor.screen, state, *code == KeyCode::PageDown)
                }
                KeyCode::ArrowLeft | KeyCode::ArrowRight if screen.track_group.is_some() => {
                    layout.page_button(cursor.screen, state, *code == KeyCode::ArrowRight)
                }
                KeyCode::ArrowUp | KeyCode::ArrowDown if screen.track_group.is_some() => {
                    self.step_gallery_track(*code == KeyCode::ArrowDown, false)?;
                    return Ok(true);
                }
                KeyCode::ArrowLeft | KeyCode::ArrowRight | KeyCode::ArrowUp | KeyCode::ArrowDown => {
                    let direction = match code {
                        KeyCode::ArrowLeft => GalleryDirection::Left,
                        KeyCode::ArrowRight => GalleryDirection::Right,
                        KeyCode::ArrowUp => GalleryDirection::Up,
                        _ => GalleryDirection::Down,
                    };
                    cursor.focus = screen.navigate(cursor.focus, direction, state);
                    None
                }
                _ => None,
            },
            _ => None,
        };
        if let Some(index) = action {
            self.activate_authored_gallery(index)?;
        }
        Ok(true)
    }

    fn activate_authored_gallery(&mut self, index: usize) -> SessionResult<()> {
        let cursor = self.gallery_cursor.as_mut().expect("active gallery cursor");
        let layout = self.vm.authored_gallery().expect("active gallery layout");
        let state = self.vm.state();
        let screen = &layout.screens[cursor.screen];
        let button = &screen.buttons[index];
        if !button.enabled(state) {
            return Ok(());
        }
        match &button.action {
            GalleryAction::Open { screen } => {
                cursor.open(layout.screen_index(screen).expect("validated screen"), layout, state);
            }
            GalleryAction::NextPage { screen } | GalleryAction::PreviousPage { screen } => {
                cursor.switch(layout.screen_index(screen).expect("validated screen"), layout, state);
            }
            GalleryAction::Back => self.authored_gallery_back()?,
            GalleryAction::View { .. } => cursor.detail = Some((index, 0)),
            GalleryAction::Script { resource } => self.start_menu_script(resource.clone())?,
            GalleryAction::Track { resource, select_only } => {
                cursor.selected_track = Some((cursor.screen, index));
                if !select_only {
                    cursor.playing_track = cursor.selected_track;
                    let commands = self.vm.play_gallery_track(resource.clone()).map_err(vm_error)?;
                    self.audio.apply(commands)?;
                }
            }
            GalleryAction::PlaySelected => self.play_selected_track()?,
            GalleryAction::Stop => {
                cursor.playing_track = None;
                let commands = self.vm.gallery_bgm_stop().map_err(vm_error)?;
                self.audio.apply(commands)?;
            }
            GalleryAction::PreviousTrack | GalleryAction::NextTrack => {
                let next = matches!(button.action, GalleryAction::NextTrack);
                self.step_gallery_track(next, true)?;
            }
        }
        Ok(())
    }

    fn play_selected_track(&mut self) -> SessionResult<()> {
        let cursor = self.gallery_cursor.as_mut().expect("active gallery cursor");
        let layout = self.vm.authored_gallery().expect("active gallery layout");
        if let Some((sheet, track)) = cursor.selected_track {
            let track = &layout.screens[sheet].buttons[track];
            if track.enabled(self.vm.state())
                && let GalleryAction::Track { resource, .. } = &track.action
            {
                cursor.playing_track = cursor.selected_track;
                let commands = self.vm.play_gallery_track(resource.clone()).map_err(vm_error)?;
                self.audio.apply(commands)?;
            }
        }
        Ok(())
    }

    fn step_gallery_track(&mut self, next: bool, play_if_active: bool) -> SessionResult<()> {
        let cursor = self.gallery_cursor.as_mut().expect("active gallery cursor");
        let layout = self.vm.authored_gallery().expect("active gallery layout");
        let state = self.vm.state();
        let selected = cursor.selected_track.map(|(_, track)| track);
        if let Some((sheet, track)) = layout.step_track(cursor.screen, selected, state, next) {
            if cursor.screen != sheet {
                cursor.switch(sheet, layout, state);
            }
            cursor.selected_track = Some((sheet, track));
            if play_if_active && state.audio.get(&0).is_some_and(|stream| stream.playing) {
                self.play_selected_track()?;
            }
        }
        Ok(())
    }

    fn step_gallery_image(&mut self, forward: bool) {
        let cursor = self.gallery_cursor.as_mut().expect("active gallery cursor");
        let layout = self.vm.authored_gallery().expect("active gallery layout");
        if let Some((button, image)) = cursor.detail
            && let GalleryAction::View { images } = &layout.screens[cursor.screen].buttons[button].action
        {
            cursor.detail = if forward {
                (image + 1 < images.len()).then_some((button, image + 1))
            } else {
                Some((button, image.saturating_sub(1)))
            };
        }
    }

    fn authored_gallery_back(&mut self) -> SessionResult<()> {
        let cursor = self.gallery_cursor.as_mut().expect("active gallery cursor");
        if cursor.detail.is_some() {
            cursor.back();
            return Ok(());
        }
        let commands = self.vm.gallery_bgm_stop().map_err(vm_error)?;
        self.audio.apply(commands)?;
        if !cursor.back() {
            self.gallery_cursor = None;
            let focus = self.title_focus.unwrap_or(0);
            self.vm.set_gallery_page(MusicaSystemPage::Title, focus).map_err(vm_error)?;
        }
        Ok(())
    }
}
