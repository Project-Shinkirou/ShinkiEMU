use super::*;
use serde::{Deserialize, Serialize};
use shinki_emu_sdk::runtime::SessionResult;
use std::collections::BTreeSet;

use crate::scene::{core_error, error};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GalleryLayout {
    pub screens: Vec<GalleryScreen>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GalleryScreen {
    pub id: String,
    pub layers: Vec<GalleryLayer>,
    pub buttons: Vec<GalleryButton>,
    pub hover: Option<String>,
    pub selection: Option<GalleryLayer>,
    pub playing: Option<GalleryLayer>,
    pub initial_focus: Option<usize>,
    pub initial_selection: Option<usize>,
    pub track_group: Option<String>,
    #[serde(default)]
    pub wrap_focus: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GalleryLayer {
    pub resource: String,
    #[serde(default)]
    pub at: [i32; 2],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GalleryButton {
    pub rect: [i32; 4],
    pub action: GalleryAction,
    #[serde(default)]
    pub requires: BTreeMap<String, i64>,
    pub hover: Option<String>,
    pub thumbnail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum GalleryAction {
    Open { screen: String },
    PreviousPage { screen: String },
    NextPage { screen: String },
    Back,
    View { images: Vec<String> },
    Script { resource: String },
    Track { resource: String, #[serde(default)] select_only: bool },
    PlaySelected,
    Stop,
    PreviousTrack,
    NextTrack,
}

#[derive(Clone, Copy)]
pub(crate) enum GalleryDirection {
    Left,
    Right,
    Up,
    Down,
}

pub(crate) struct GalleryCursor {
    pub screen: usize,
    pub focus: Option<usize>,
    pub selected_track: Option<(usize, usize)>,
    pub playing_track: Option<(usize, usize)>,
    pub detail: Option<(usize, usize)>,
    pub pressed: Option<usize>,
    parents: Vec<(usize, Option<usize>)>,
}

impl GalleryLayout {
    pub(in crate::runtime) fn load(
        root: &std::path::Path,
        path: &str,
        archive: &crate::MusicaMountedVfs,
        width: u32,
        height: u32,
    ) -> SessionResult<Self> {
        let bytes = crate::resources::read_resource(root, path, 1024 * 1024).map_err(core_error)?;
        let layout: Self = serde_json::from_slice(&bytes)
            .map_err(|_| layout_error("invalid authored gallery JSON"))?;
        layout.validate(width, height, |uri| {
            let stat = archive.stat(uri).map_err(core_error)?;
            if stat.kind != crate::ArchiveNodeKind::File {
                return Err(layout_error("gallery resource must be a file"));
            }
            Ok(())
        })?;
        Ok(layout)
    }

    pub(crate) fn screen_index(&self, name: &str) -> Option<usize> {
        self.screens.iter().position(|sheet| sheet.id == name)
    }

    pub(crate) fn page_button(
        &self,
        screen: usize,
        state: &MusicaRuntimeState,
        next: bool,
    ) -> Option<usize> {
        self.screens[screen].buttons.iter().position(|button| {
            button.enabled(state)
                && if next {
                    matches!(button.action, GalleryAction::NextPage { .. })
                } else {
                    matches!(button.action, GalleryAction::PreviousPage { .. })
                }
        })
    }

    pub(crate) fn step_track(
        &self,
        screen: usize,
        selected: Option<usize>,
        state: &MusicaRuntimeState,
        next: bool,
    ) -> Option<(usize, usize)> {
        let group = self.screens[screen].track_group.as_ref();
        let mut tracks = self.screens.iter().enumerate()
            .filter(|(i, sheet)| *i == screen || (group.is_some() && sheet.track_group.as_ref() == group))
            .flat_map(|(i, sheet)| sheet.buttons.iter().enumerate().filter_map(move |(j, button)| {
                (matches!(button.action, GalleryAction::Track { .. }) && button.enabled(state))
                    .then_some((i, j))
            }));
        let edge = if next { tracks.next() } else { tracks.next_back() }?;
        let after_cursor = |&(i, j): &(usize, usize)| {
            if next {
                i > screen || (i == screen && selected.is_none_or(|at| j > at))
            } else {
                i < screen || (i == screen && selected.is_none_or(|at| j < at))
            }
        };
        if after_cursor(&edge) {
            return Some(edge);
        }
        Some(if next { tracks.find(after_cursor) } else { tracks.rfind(after_cursor) }.unwrap_or(edge))
    }

    fn validate(
        &self,
        width: u32,
        height: u32,
        mut resource: impl FnMut(&str) -> SessionResult<()>,
    ) -> SessionResult<()> {
        if self.screens.is_empty()
            || self.screens.len() > 256
            || self.screens.iter().map(|s| s.buttons.len()).sum::<usize>() > 4096
        {
            return Err(layout_error("invalid authored gallery screen or button count"));
        }
        let mut ids = BTreeSet::new();
        for screen in &self.screens {
            if screen.id.is_empty() || !ids.insert(&screen.id) || screen.buttons.is_empty() {
                return Err(layout_error("gallery screens must have unique IDs and buttons"));
            }
        }
        for screen in &self.screens {
            if screen.layers.is_empty() || screen.layers.len() > 32 {
                return Err(layout_error("invalid gallery layer count"));
            }
            for layer in screen.layers.iter().chain(&screen.selection).chain(&screen.playing) {
                resource(&layer.resource)?;
            }
            if let Some(uri) = &screen.hover {
                resource(uri)?;
            }
            if screen.initial_focus.is_some_and(|i| i >= screen.buttons.len()) {
                return Err(layout_error("initial gallery focus is outside the screen"));
            }
            if screen.initial_selection.is_some_and(|i| {
                !matches!(screen.buttons.get(i).map(|b| &b.action), Some(GalleryAction::Track { .. }))
            }) {
                return Err(layout_error("initial gallery selection must be a track button"));
            }
            for button in &screen.buttons {
                let [x, y, w, h] = button.rect;
                if x < 0 || y < 0 || w <= 0 || h <= 0
                    || i64::from(x) + i64::from(w) > i64::from(width)
                    || i64::from(y) + i64::from(h) > i64::from(height)
                {
                    return Err(layout_error("gallery button is outside the logical canvas"));
                }
                for uri in button.thumbnail.iter().chain(&button.hover) {
                    resource(uri)?;
                }
                match &button.action {
                    GalleryAction::Open { screen: target }
                    | GalleryAction::PreviousPage { screen: target }
                    | GalleryAction::NextPage { screen: target } => {
                        if target == &screen.id || !ids.contains(target) {
                            return Err(layout_error("invalid gallery screen destination"));
                        }
                    }
                    GalleryAction::View { images } => {
                        if images.is_empty() || images.len() > 256 {
                            return Err(layout_error("invalid gallery image count"));
                        }
                        for uri in images { resource(uri)?; }
                    }
                    GalleryAction::Script { resource: uri } => {
                        if !uri.starts_with("musica:/scr/") {
                            return Err(layout_error("gallery script must use the script archive"));
                        }
                        resource(uri)?;
                    }
                    GalleryAction::Track { resource: uri, .. } => {
                        if !uri.starts_with("musica:/bgm/") {
                            return Err(layout_error("gallery track must use the music archive"));
                        }
                        resource(uri)?;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

impl GalleryButton {
    pub(crate) fn enabled(&self, state: &MusicaRuntimeState) -> bool {
        super::super::title::unlocked(&self.requires, state)
    }
}

impl GalleryScreen {
    pub(crate) fn hit(&self, x: f32, y: f32, state: &MusicaRuntimeState) -> Option<usize> {
        self.buttons.iter().position(|button| {
            let [left, top, width, height] = button.rect;
            (left as f32..(left + width) as f32).contains(&x)
                && (top as f32..(top + height) as f32).contains(&y)
                && button.enabled(state)
        })
    }

    pub(crate) fn navigate(
        &self,
        from: Option<usize>,
        direction: GalleryDirection,
        state: &MusicaRuntimeState,
    ) -> Option<usize> {
        let buttons = self.buttons.iter().enumerate().filter(|(_, b)| b.enabled(state));
        let Some((from, origin)) = from.and_then(|i| self.buttons.get(i).map(|b| (i, b.rect))) else {
            return buttons.map(|(i, _)| i).next();
        };
        let [x, y, w, h] = origin.map(i64::from);
        buttons.filter_map(|(i, button)| {
            let [a, b, c, d] = button.rect.map(i64::from);
            let dx = 2 * (a - x) + c - w;
            let dy = 2 * (b - y) + d - h;
            let (forward, cross, aligned) = match direction {
                GalleryDirection::Left => (-dx, dy, y < b + d && b < y + h),
                GalleryDirection::Right => (dx, dy, y < b + d && b < y + h),
                GalleryDirection::Up => (-dy, dx, x < a + c && a < x + w),
                GalleryDirection::Down => (dy, dx, x < a + c && a < x + w),
            };
            let wraps = forward < 0 && aligned && self.wrap_focus;
            (forward > 0 || wraps)
                .then_some((!aligned, wraps, forward + cross.abs(), cross.abs(), i))
        }).min().map(|(_, _, _, _, i)| i).or(Some(from))
    }
}

impl GalleryCursor {
    pub(crate) fn new(layout: &GalleryLayout, state: &MusicaRuntimeState) -> Self {
        let mut cursor = Self {
            screen: 0,
            focus: None,
            selected_track: None,
            playing_track: None,
            detail: None,
            pressed: None,
            parents: Vec::new(),
        };
        cursor.switch(0, layout, state);
        cursor
    }

    pub(crate) fn open(&mut self, index: usize, layout: &GalleryLayout, state: &MusicaRuntimeState) {
        // Re-entering an ancestor unwinds to it instead of growing a cyclic stack.
        if let Some(position) = self.parents.iter().position(|(screen, _)| *screen == index) {
            let (_, focus) = self.parents[position];
            self.parents.truncate(position);
            self.switch(index, layout, state);
            self.focus = focus;
        } else {
            self.parents.push((self.screen, self.focus));
            self.switch(index, layout, state);
        }
    }

    pub(crate) fn switch(&mut self, index: usize, layout: &GalleryLayout, state: &MusicaRuntimeState) {
        self.screen = index;
        self.detail = None;
        self.pressed = None;
        let screen = &layout.screens[index];
        self.focus = screen.initial_focus.filter(|&i| screen.buttons[i].enabled(state));
        self.selected_track = screen.initial_selection
            .filter(|&i| screen.buttons[i].enabled(state))
            .map(|i| (index, i));
    }

    pub(crate) fn back(&mut self) -> bool {
        self.pressed = None;
        if self.detail.take().is_some() {
            return true;
        }
        if let Some((screen, focus)) = self.parents.pop() {
            self.screen = screen;
            self.focus = focus;
            self.selected_track = None;
            self.playing_track = None;
            return true;
        }
        false
    }
}

fn layout_error(message: &str) -> shinki_emu_sdk::runtime::SessionError {
    error("SHINKI_EMU_MUSICA_GALLERY_LAYOUT", message)
}
