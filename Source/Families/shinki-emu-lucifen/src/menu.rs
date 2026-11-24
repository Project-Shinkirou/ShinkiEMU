use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    sync::Arc,
};

use shinki_emu_lucifen_formats::{Lgm, member_key};

use crate::{GameAssets, Graphics, Surface, Word, graphics::BitmapTarget, input::Input};

pub(crate) struct MenuControl {
    pub(crate) handle: i32,
    pub(crate) extent: [i16; 2],
    pub(crate) states: u8,
}

struct Button {
    group: Word,
    atlas: i32,
    owns_atlas: bool,
    width: i32,
    height: i32,
    frames: u8,
    kind: u8,
    frame: u8,
    enabled: bool,
    rectangular_hit: bool,
    selected: bool,
    radio_root: Option<i32>,
    command: i32,
    tooltip: Vec<u8>,
}

pub(crate) struct DirectButton {
    pub(crate) atlas: i32,
    pub(crate) owns_atlas: bool,
    pub(crate) position: [i32; 2],
    pub(crate) press_frame: bool,
    pub(crate) disabled_frame: bool,
    pub(crate) blend: u8,
    pub(crate) priority: u32,
}

pub(crate) struct TextList {
    group: Word,
    pub(crate) position: [i32; 2],
    pub(crate) layout: [i32; 3],
    pub(crate) font: i32,
    pub(crate) priority: u32,
    pub(crate) overlay: bool,
    pub(crate) visible: bool,
    rows: Vec<i32>,
    annotation_key: Option<(u8, u16)>,
    command: i32,
    pub(crate) text_colors: [u32; 4],
    pub(crate) gradients: [[u32; 2]; 4],
    pub(crate) highlight: (u8, u32),
    pub(crate) padding: [i32; 4],
    pub(crate) text_inset: [i32; 2],
    pub(crate) background: [u8; 4],
    pub(crate) skin: Option<ChoiceSkin>,
}

#[derive(Clone)]
pub(crate) struct ChoiceSkin {
    pub(crate) bitmap: Arc<Surface>,
    pub(crate) frames: u8,
    pub(crate) disabled_frame: bool,
    pub(crate) source: ScrollSkin,
}

struct ScrollControl {
    group: Word,
    bitmap: i32,
    font: i32,
    text_layout: ScrollTextLayout,
    is_backlog: bool,
    text: String,
    companion: Option<(i32, u8)>,
    skin: Option<Arc<Surface>>,
    skin_source: ScrollSkin,
    cursor_strip: i32,
    history_toggle: Option<i32>,
    message_active: bool,
    automatic_message: bool,
    caret: [i32; 2],
    line_height: i32,
}

#[derive(Clone, Copy)]
pub(crate) struct ScrollTextLayout {
    pub(crate) flow: [i32; 6],
    pub(crate) padding: [i32; 4],
    pub(crate) overhang: [i32; 4],
}

struct Hotspot {
    group: Word,
    rect: [i32; 4],
    center: [i32; 2],
    mouse: bool,
    can_activate: bool,
    enabled: bool,
    command: i32,
}

pub(crate) struct ScrollSetup {
    pub(crate) position: [i32; 2],
    pub(crate) size: [u32; 2],
    pub(crate) font: i32,
    pub(crate) priority: u32,
    pub(crate) is_backlog: bool,
    pub(crate) skin: Option<Arc<Surface>>,
    pub(crate) skin_source: ScrollSkin,
}

#[derive(Clone, Default)]
pub(crate) struct ScrollSkin {
    pub(crate) base: Option<Arc<Surface>>,
    pub(crate) primary_mask: Option<Arc<Surface>>,
    pub(crate) secondary_mask: Option<Arc<Surface>>,
}

impl ScrollSkin {
    pub(crate) fn render(&self, mode: bool, color: [u8; 4]) -> io::Result<Option<Arc<Surface>>> {
        let mask = if mode {
            self.primary_mask.as_deref()
        } else {
            self.secondary_mask.as_deref()
        };
        Graphics::text_window_skin(self.base.clone(), mask, color)
    }
}

pub(crate) struct Menu {
    pub(crate) controls: Vec<MenuControl>,
    buttons: BTreeMap<i32, Button>,
    pub(crate) text_lists: BTreeMap<i32, TextList>,
    scroll_controls: BTreeMap<i32, ScrollControl>,
    hotspots: BTreeMap<i32, Hotspot>,
    active_text_list: Option<i32>,
    routing: Vec<i32>,
    routing_stack: Vec<Vec<i32>>,
    current_group: Word,
    overlay: bool,
    focus: Option<i32>,
    pressed: Option<i32>,
    pointer: Option<[i32; 2]>,
    command: i32,
}

impl Default for Menu {
    fn default() -> Self {
        Self {
            controls: Vec::new(),
            buttons: BTreeMap::new(),
            text_lists: BTreeMap::new(),
            scroll_controls: BTreeMap::new(),
            hotspots: BTreeMap::new(),
            active_text_list: None,
            routing: Vec::new(),
            routing_stack: Vec::new(),
            current_group: Word::Integer(0),
            overlay: false,
            focus: None,
            pressed: None,
            pointer: None,
            command: -1,
        }
    }
}

impl Button {
    fn sprite_frame(&self, focused: bool, pressed: bool) -> u8 {
        let state = if !self.enabled {
            3
        } else if focused {
            if pressed { 2 } else { 1 }
        } else {
            0
        };
        match self.kind {
            1 | 2 if self.frames == 3 && state == 3 => 0,
            1 | 2 => state,
            3 if self.frames == 2 && state == 3 => 0,
            3 => state.min(1) + u8::from(state == 3),
            4..=9 => {
                let (base, cells) = match self.kind {
                    4 => (6, [0, 1, 2, 3, 4, 5, 6, 7]),
                    5 => (4, [0, 1, 1, 2, 3, 3, 4, 5]),
                    6 | 8 => (4, [0, 1, 2, 3, 3, 3, 4, 5]),
                    _ => (3, [0, 1, 1, 2, 2, 2, 3, 4]),
                };
                let selected = usize::from(self.selected);
                let state = if state == 3 && self.frames > base {
                    6 + selected
                } else {
                    usize::from(if state == 3 { 0 } else { state }) + 3 * selected
                };
                cells[state]
            }
            _ => state,
        }
    }
}

impl Menu {
    pub(crate) fn clear(&mut self) {
        self.controls.clear();
    }

    pub(crate) fn reset_input(&mut self) {
        self.focus = None;
        self.pressed = None;
        self.pointer = None;
        self.command = -1;
    }

    pub(crate) fn take_command(&mut self) -> i32 {
        std::mem::replace(&mut self.command, -1)
    }

    pub(crate) fn free(&mut self, handle: i32, graphics: &mut Graphics) -> bool {
        if let Some(button) = self.buttons.remove(&handle) {
            graphics.free_texture(handle);
            if button.owns_atlas {
                graphics.free(button.atlas);
            }
        } else if let Some(list) = self.text_lists.remove(&handle) {
            for row in list.rows {
                self.free(row, graphics);
            }
        } else if let Some(control) = self.scroll_controls.remove(&handle) {
            graphics.free_texture(handle);
            graphics.free(control.bitmap);
            for related in self.scroll_controls.values_mut() {
                if related.companion.is_some_and(|(id, _)| id == handle) {
                    related.companion = None;
                }
            }
        } else if self.hotspots.remove(&handle).is_none() {
            return false;
        }
        if self.active_text_list == Some(handle) {
            self.active_text_list = None;
        }
        self.detach_input(handle);
        true
    }

    pub(crate) fn detach_input(&mut self, handle: i32) {
        self.routing.retain(|&entry| entry != handle);
        for routing in &mut self.routing_stack {
            routing.retain(|&entry| entry != handle);
        }
        if self.focus == Some(handle) {
            self.focus = None;
            self.pointer = None;
        }
        if self.pressed == Some(handle) {
            self.pressed = None;
        }
    }

    pub(crate) fn attach_input(&mut self, handle: i32, index: i32) -> i32 {
        if handle == 0 {
            return -1;
        }
        if !self.buttons.contains_key(&handle)
            && !self.text_lists.contains_key(&handle)
            && !self.hotspots.contains_key(&handle)
            && !self.scroll_controls.contains_key(&handle)
        {
            return 1;
        }
        if self.routing.contains(&handle)
            || self
                .routing_stack
                .iter()
                .any(|routing| routing.contains(&handle))
        {
            return 2;
        }
        self.routing
            .insert((index as usize).min(self.routing.len()), handle);
        self.pointer = None;
        0
    }

    pub(crate) fn clear_routing(&mut self) {
        self.routing.clear();
        self.reset_input();
    }

    pub(crate) fn push_routing(&mut self) -> i32 {
        if self.routing.is_empty() {
            return 1;
        }
        self.routing_stack.push(std::mem::take(&mut self.routing));
        self.reset_input();
        0
    }

    pub(crate) fn pop_routing(&mut self) -> i32 {
        let Some(previous) = self.routing_stack.pop() else {
            return 1;
        };
        self.routing = previous;
        self.reset_input();
        0
    }

    pub(crate) fn select_group(&mut self, group: Word) {
        self.current_group = group;
    }

    pub(crate) fn select_overlay(&mut self, overlay: bool) -> i32 {
        i32::from(std::mem::replace(&mut self.overlay, overlay))
    }

    pub(crate) fn free_group(&mut self, group: Word, keep_group: bool, graphics: &mut Graphics) {
        let handles: Vec<_> = self
            .buttons
            .iter()
            .filter_map(|(&handle, button)| {
                ((button.group == group) != keep_group).then_some(handle)
            })
            .chain(self.text_lists.iter().filter_map(|(&handle, list)| {
                ((list.group == group) != keep_group).then_some(handle)
            }))
            .chain(
                self.scroll_controls
                    .iter()
                    .filter_map(|(&handle, control)| {
                        ((control.group == group) != keep_group).then_some(handle)
                    }),
            )
            .chain(self.hotspots.iter().filter_map(|(&handle, hotspot)| {
                ((hotspot.group == group) != keep_group).then_some(handle)
            }))
            .collect();
        for handle in handles {
            self.free(handle, graphics);
        }
    }

    pub(crate) fn free_all(&mut self, graphics: &mut Graphics) {
        for (handle, button) in std::mem::take(&mut self.buttons) {
            graphics.free_texture(handle);
            if button.owns_atlas {
                graphics.free(button.atlas);
            }
        }
        self.text_lists.clear();
        self.hotspots.clear();
        for (handle, control) in std::mem::take(&mut self.scroll_controls) {
            graphics.free_texture(handle);
            graphics.free(control.bitmap);
        }
        self.active_text_list = None;
        self.clear();
        self.clear_routing();
        self.routing_stack.clear();
    }

    pub(crate) fn create_direct_button(
        &mut self,
        setup: DirectButton,
        graphics: &mut Graphics,
    ) -> io::Result<i32> {
        let Ok(atlas) = graphics.surface(setup.atlas) else {
            return Ok(0);
        };
        let frames = 2 + u8::from(setup.press_frame) + u8::from(setup.disabled_frame);
        let width = (atlas.width / u32::from(frames)) as i32;
        let height = atlas.height as i32;
        if width == 0 {
            return Ok(0);
        }
        let texture = graphics.texture_from_surface(
            setup.atlas,
            Some([0, 0, width, height]),
            setup.position,
        )?;
        graphics.texture_blend(texture, setup.blend);
        if graphics.attach_layer(
            graphics.display(),
            texture,
            setup.priority,
            0,
            self.overlay,
            false,
        ) != 0
        {
            graphics.free_texture(texture);
            return Ok(0);
        }
        self.buttons.insert(
            texture,
            Button {
                group: self.current_group,
                atlas: setup.atlas,
                owns_atlas: setup.owns_atlas,
                width,
                height,
                frames,
                kind: if setup.press_frame { 1 } else { 3 },
                frame: 0,
                enabled: true,
                rectangular_hit: setup.blend == 0,
                selected: false,
                radio_root: None,
                command: -1,
                tooltip: Vec::new(),
            },
        );
        Ok(texture)
    }

    pub(crate) fn create_hotspot(
        &mut self,
        graphics: &mut Graphics,
        rect: [i32; 4],
        center: [i32; 2],
        mouse: bool,
        can_activate: bool,
    ) -> io::Result<i32> {
        let [x, y, width, height] = rect;
        if width <= 0 || height <= 0 {
            return Ok(0);
        }
        let handle = graphics.next_surface()?;
        self.hotspots.insert(
            handle,
            Hotspot {
                group: self.current_group,
                rect,
                center: [
                    if center[0] < 0 {
                        x.saturating_add(width / 2)
                    } else {
                        center[0]
                    },
                    if center[1] < 0 {
                        y.saturating_add(height / 2)
                    } else {
                        center[1]
                    },
                ],
                mouse,
                can_activate,
                enabled: true,
                command: -1,
            },
        );
        Ok(handle)
    }

    pub(crate) fn move_hotspot(&mut self, handle: i32, position: [i32; 2]) -> bool {
        let Some(hotspot) = self.hotspots.get_mut(&handle) else {
            return false;
        };
        hotspot.rect[..2].copy_from_slice(&position);
        true
    }

    pub(crate) fn set_hotspot_center(&mut self, handle: i32, center: [i32; 2]) -> bool {
        let Some(hotspot) = self.hotspots.get_mut(&handle) else {
            return false;
        };
        hotspot.center = center;
        true
    }

    pub(crate) fn create_text_list(
        &mut self,
        graphics: &mut Graphics,
        position: [i32; 2],
        layout: [i32; 3],
        font: i32,
        priority: u32,
    ) -> io::Result<i32> {
        let handle = graphics.next_surface()?;
        self.text_lists.insert(
            handle,
            TextList {
                group: self.current_group,
                position,
                layout,
                font,
                priority,
                overlay: self.overlay,
                visible: true,
                rows: Vec::new(),
                annotation_key: None,
                command: -1,
                text_colors: [0xffffff, 9428959, 0xfff000, 0x909090],
                gradients: [
                    [16771840, 16729670],
                    [65515, 2546982],
                    [16777030, 16749202],
                    [11579568, 5855577],
                ],
                highlight: (1, 0x80000080),
                padding: [8, 2, 8, 2],
                text_inset: [2, 2],
                background: [0, 0, 0, 128],
                skin: None,
            },
        );
        Ok(handle)
    }

    pub(crate) fn activate_text_list(&mut self, handle: i32) -> i32 {
        self.bind_text_list_annotations(None);
        self.active_text_list = self.text_lists.contains_key(&handle).then_some(handle);
        i32::from(self.active_text_list.is_none())
    }

    pub(crate) fn bind_text_list_annotations(&mut self, key: Option<(u8, u16)>) {
        if let Some(list) = self
            .active_text_list
            .and_then(|id| self.text_lists.get_mut(&id))
        {
            list.annotation_key = key.filter(|(group, _)| *group != 2);
        }
    }

    pub(crate) fn copy_text_list_annotations(&mut self, handle: i32) -> i32 {
        let source = self
            .active_text_list
            .and_then(|id| self.text_lists.get(&id));
        let key = source.map(|list| list.annotation_key);
        let Some(destination) = self.text_lists.get_mut(&handle) else {
            return -1;
        };
        let Some(key) = key else {
            return 1;
        };
        destination.annotation_key = key;
        0
    }

    pub(crate) fn set_choice_skin(&mut self, handle: i32, skin: ChoiceSkin) -> i32 {
        let Some(list) = self.text_lists.get_mut(&handle) else {
            return 4;
        };
        let width = skin.bitmap.width / u32::from(skin.frames);
        if width == 0 {
            return 1;
        }
        list.layout[0] = width as i32;
        list.layout[1] = skin.bitmap.height as i32;
        list.skin = Some(skin);
        0
    }

    pub(crate) fn create_scroll_control(
        &mut self,
        graphics: &mut Graphics,
        setup: ScrollSetup,
    ) -> io::Result<i32> {
        let bitmap = graphics.create(setup.size[0], setup.size[1])?;
        if let Some(skin) = &setup.skin {
            graphics.stamp_bitmap(
                BitmapTarget::Surface(bitmap),
                skin,
                [0, 0, i64::from(skin.width), i64::from(skin.height)],
                [0, 0],
                false,
            );
        }
        let result = (|| {
            let texture = graphics.texture_from_surface(bitmap, None, setup.position)?;
            if graphics.attach_layer(
                graphics.display(),
                texture,
                setup.priority,
                0,
                self.overlay,
                false,
            ) != 0
            {
                graphics.free_texture(texture);
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid Lucifen scroll control target",
                ));
            }
            self.scroll_controls.insert(
                texture,
                ScrollControl {
                    group: self.current_group,
                    bitmap,
                    font: setup.font,
                    text_layout: ScrollTextLayout {
                        flow: [0; 6],
                        padding: [8; 4],
                        overhang: [2; 4],
                    },
                    is_backlog: setup.is_backlog,
                    text: String::new(),
                    companion: None,
                    skin: setup.skin,
                    skin_source: setup.skin_source,
                    cursor_strip: 0,
                    history_toggle: None,
                    message_active: false,
                    automatic_message: false,
                    caret: [8, 8],
                    line_height: 0,
                },
            );
            Ok(texture)
        })();
        if result.is_err() {
            graphics.free(bitmap);
        }
        result
    }

    pub(crate) fn scroll_font(&self, handle: i32) -> Option<i32> {
        self.scroll_controls
            .get(&handle)
            .map(|control| control.font)
    }

    pub(crate) fn is_message_window(&self, handle: i32) -> bool {
        self.scroll_controls
            .get(&handle)
            .is_some_and(|control| control.is_backlog)
    }

    pub(crate) fn message_active(&self, handle: i32) -> bool {
        self.scroll_controls
            .get(&handle)
            .is_some_and(|control| control.message_active)
    }

    pub(crate) fn message_automatic(&self, handle: i32) -> bool {
        self.scroll_controls
            .get(&handle)
            .is_some_and(|control| control.automatic_message)
    }

    pub(crate) fn set_message_automatic(&mut self, handle: i32, automatic: bool) {
        if let Some(control) = self.scroll_controls.get_mut(&handle) {
            control.automatic_message = automatic;
        }
    }

    pub(crate) fn set_scroll_caret(&mut self, handle: i32, position: [i32; 2], line_height: i32) {
        if let Some(control) = self.scroll_controls.get_mut(&handle) {
            control.caret = position;
            control.line_height = line_height;
        }
    }

    pub(crate) fn scroll_caret(&self, handle: i32, height: i32) -> Option<[i32; 2]> {
        let control = self.scroll_controls.get(&handle)?;
        let [x, y] = control.caret;
        Some([
            x,
            y.saturating_add(control.line_height).saturating_sub(height),
        ])
    }

    pub(crate) fn text_row_caret(
        &self,
        handle: i32,
        index: i32,
        graphics: &Graphics,
    ) -> Option<[i32; 2]> {
        let list = self.text_lists.get(&handle)?;
        let texture = *list.rows.get(index as usize)?;
        let [x, y, _, bottom] = graphics.texture_bounds(texture)?;
        Some([x + list.padding[0] - 8, y + (bottom - y) / 2])
    }

    pub(crate) fn reset_scroll_text(&mut self, handle: i32, graphics: &mut Graphics) -> bool {
        let Some(control) = self.scroll_controls.get_mut(&handle) else {
            return false;
        };
        control.text.clear();
        let [left, top, _, _] = control.text_layout.padding;
        control.caret = [left, top];
        control.message_active = false;
        control.automatic_message = false;
        Self::restore_scroll_bitmap(control, graphics);
        if let Some((companion, _)) = control.companion {
            graphics.show_texture(companion, false);
        }
        true
    }

    pub(crate) fn message_companion(&self, handle: i32) -> Option<(i32, u8)> {
        self.scroll_controls.get(&handle)?.companion
    }

    pub(crate) fn set_control_value(&mut self, handle: i32, value: i32) -> i32 {
        let Some(button) = self.buttons.get_mut(&handle) else {
            return -1;
        };
        match button.kind {
            1 => i32::from(std::mem::replace(&mut button.selected, value != 0) != (value != 0)),
            3 | 4 => {
                button.selected = value != 0;
                0
            }
            _ => -1,
        }
    }

    pub(crate) fn set_message_appearance(
        &mut self,
        scroll: i32,
        list: i32,
        mode: bool,
        color: [u8; 4],
        refresh: bool,
        graphics: &mut Graphics,
    ) -> io::Result<()> {
        if let Some(control) = self.scroll_controls.get_mut(&scroll)
            && let Some(skin) = control.skin_source.render(mode, color)?
        {
            control.skin = Some(skin);
            if refresh {
                Self::restore_scroll_bitmap(control, graphics);
            }
        }
        if let Some(list) = self.text_lists.get_mut(&list) {
            list.background = color;
            if let Some(skin) = &mut list.skin
                && let Some(bitmap) = skin.source.render(mode, color)?
            {
                skin.bitmap = bitmap;
            }
        }
        Ok(())
    }

    pub(crate) fn link_scroll_controls(&mut self, backlog: i32, companion: i32, mode: u8) -> i32 {
        if self
            .scroll_controls
            .get(&backlog)
            .is_none_or(|control| !control.is_backlog)
        {
            return 1;
        }
        if self
            .scroll_controls
            .get(&companion)
            .is_none_or(|control| control.is_backlog)
        {
            return 2;
        }
        self.scroll_controls.get_mut(&backlog).unwrap().companion = Some((companion, mode));
        0
    }

    pub(crate) fn set_scroll_cursor(&mut self, handle: i32, strip: i32) -> bool {
        let Some(control) = self
            .scroll_controls
            .get_mut(&handle)
            .filter(|control| control.is_backlog)
        else {
            return false;
        };
        control.cursor_strip = strip;
        true
    }

    pub(crate) fn set_history_toggle(&mut self, handle: i32, toggle: i32) -> i32 {
        let Some(control) = self
            .scroll_controls
            .get_mut(&handle)
            .filter(|control| control.is_backlog)
        else {
            return 1;
        };
        if self
            .buttons
            .get(&toggle)
            .is_none_or(|button| button.kind < 4)
        {
            return 2;
        }
        control.history_toggle = Some(toggle);
        0
    }

    pub(crate) fn set_scroll_flow(&mut self, handle: i32, flow: [i32; 6]) -> bool {
        let Some(control) = self.scroll_controls.get_mut(&handle) else {
            return false;
        };
        control.text_layout.flow = flow;
        true
    }

    pub(crate) fn set_scroll_text_padding(
        &mut self,
        handle: i32,
        padding: [i32; 4],
        overhang: [i32; 4],
        graphics: &mut Graphics,
    ) -> bool {
        let Some(control) = self.scroll_controls.get_mut(&handle) else {
            return false;
        };
        control.text_layout.padding = padding;
        control.text_layout.overhang = overhang;
        self.reset_scroll_text(handle, graphics)
    }

    pub(crate) fn update_scroll_text(
        &mut self,
        handle: i32,
        text: &str,
        replace: bool,
        graphics: &mut Graphics,
    ) -> Option<(i32, i32, ScrollTextLayout, &str)> {
        let control = self.scroll_controls.get_mut(&handle)?;
        if control.is_backlog && !replace {
            return None;
        }
        if replace {
            control.text.clear();
            let [left, top, _, _] = control.text_layout.padding;
            control.caret = [left, top];
            control.message_active = control.is_backlog;
            graphics.show_texture(handle, true);
        }
        control.text.push_str(text);
        Self::restore_scroll_bitmap(control, graphics);
        Some((
            control.bitmap,
            control.font,
            control.text_layout,
            &control.text,
        ))
    }

    fn restore_scroll_bitmap(control: &ScrollControl, graphics: &mut Graphics) {
        if let Some(skin) = &control.skin {
            graphics.stamp_bitmap(
                BitmapTarget::Surface(control.bitmap),
                skin,
                [0, 0, i64::from(skin.width), i64::from(skin.height)],
                [0, 0],
                false,
            );
        } else {
            graphics.fill(control.bitmap, None, [0, 0, 0, 0]);
        }
    }

    pub(crate) fn show_text_list(
        &mut self,
        handle: i32,
        visible: bool,
        graphics: &mut Graphics,
    ) -> bool {
        if let Some((companion, _)) = self
            .scroll_controls
            .get(&handle)
            .and_then(|control| control.companion)
        {
            graphics.show_texture(companion, false);
        }
        let Some(list) = self.text_lists.get_mut(&handle) else {
            return false;
        };
        list.visible = visible;
        for &row in &list.rows {
            graphics.show_texture(row, visible);
        }
        self.pointer = None;
        true
    }

    pub(crate) fn move_text_list(
        &mut self,
        handle: i32,
        position: [i32; 2],
        graphics: &mut Graphics,
    ) -> bool {
        let Some(list) = self.text_lists.get_mut(&handle) else {
            return false;
        };
        list.position = position;
        list.position_rows(graphics);
        self.pointer = None;
        true
    }

    pub(crate) fn append_text_row(
        &mut self,
        handle: i32,
        atlas: i32,
        disabled: bool,
        row_command: Option<i32>,
        graphics: &mut Graphics,
    ) -> io::Result<i32> {
        let list = self
            .text_lists
            .get_mut(&handle)
            .expect("validated text list");
        let [width, height, _] = list.layout;
        let texture =
            graphics.texture_from_surface(atlas, Some([0, 0, width, height]), list.position)?;
        let display = graphics.display();
        if graphics.attach_layer(display, texture, list.priority, 0, list.overlay, false) != 0 {
            graphics.free_texture(texture);
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid Lucifen text list target",
            ));
        }
        graphics.show_texture(texture, list.visible);
        let command = if let Some(command) = row_command {
            command
        } else if list.command < 0 {
            -1
        } else {
            list.command.saturating_add(list.rows.len() as i32)
        };
        self.buttons.insert(
            texture,
            Button {
                group: list.group,
                atlas,
                owns_atlas: true,
                width,
                height,
                frames: 2,
                kind: 3,
                frame: 0,
                enabled: !disabled,
                rectangular_hit: true,
                selected: false,
                radio_root: None,
                command,
                tooltip: Vec::new(),
            },
        );
        list.rows.push(texture);
        list.position_rows(graphics);
        self.pointer = None;
        Ok(0)
    }

    pub(crate) fn set_text_row_state(
        &mut self,
        handle: i32,
        index: i32,
        state: u8,
        graphics: &mut Graphics,
    ) -> io::Result<i32> {
        let Some(list) = self.text_lists.get(&handle) else {
            return Ok(-1);
        };
        let Some(row) = list
            .rows
            .get(index as usize)
            .and_then(|row| self.buttons.get_mut(row).map(|button| (*row, button)))
        else {
            return Ok(1);
        };
        let (texture, button) = row;
        let state = u8::from(state != 0);
        let left = i32::from(state) * button.width;
        graphics.set_texture_region(texture, [left, 0, left + button.width, button.height])?;
        button.frame = state;
        Ok(0)
    }

    pub(crate) fn clear_text_rows(&mut self, handle: i32, graphics: &mut Graphics) -> bool {
        let Some(list) = self.text_lists.get_mut(&handle) else {
            return false;
        };
        for row in std::mem::take(&mut list.rows) {
            self.free(row, graphics);
        }
        true
    }

    pub(crate) fn set_command(&mut self, handle: i32, command: i32) {
        if let Some(button) = self.buttons.get_mut(&handle) {
            button.command = command;
        }
        if let Some(hotspot) = self.hotspots.get_mut(&handle) {
            hotspot.command = command;
        }
        if let Some(list) = self.text_lists.get_mut(&handle) {
            list.command = command;
            for (index, id) in list.rows.iter().enumerate() {
                if let Some(row) = self.buttons.get_mut(id) {
                    row.command = if command < 0 {
                        -1
                    } else {
                        command.saturating_add(index as i32)
                    };
                }
            }
        }
    }

    pub(crate) fn set_tooltip(&mut self, handle: i32, text: Vec<u8>) {
        if let Some(button) = self.buttons.get_mut(&handle) {
            button.tooltip = text;
        }
    }

    pub(crate) fn set_enabled(&mut self, handle: i32, enabled: bool) -> i32 {
        if let Some(hotspot) = self.hotspots.get_mut(&handle) {
            return i32::from(std::mem::replace(&mut hotspot.enabled, enabled) != enabled);
        }
        let Some(button) = self.buttons.get_mut(&handle) else {
            return 0;
        };
        i32::from(std::mem::replace(&mut button.enabled, enabled) != enabled)
    }

    pub(crate) fn enabled(&self, handle: i32) -> bool {
        if let Some(hotspot) = self.hotspots.get(&handle) {
            return hotspot.enabled;
        }
        self.buttons
            .get(&handle)
            .is_some_and(|button| button.enabled)
    }

    pub(crate) fn image_handle(&self, handle: i32, atlas: bool) -> i32 {
        if handle == 0 {
            return -1;
        }
        if let Some(control) = self.scroll_controls.get(&handle) {
            return if atlas { control.bitmap } else { handle };
        }
        self.buttons
            .get(&handle)
            .map_or(0, |button| if atlas { button.atlas } else { handle })
    }

    pub(crate) fn rectangular_hit(&mut self, handle: i32, enable: bool) -> i32 {
        let Some(button) = self.buttons.get_mut(&handle) else {
            return -1;
        };
        button.rectangular_hit = enable;
        0
    }

    pub(crate) fn selection_state(&self, handle: i32) -> i32 {
        if let Some(list) = self.text_lists.get(&handle) {
            return list.rows.len() as i32;
        }
        self.buttons
            .get(&handle)
            .filter(|button| button.kind >= 4)
            .map_or(-1, |button| i32::from(button.selected))
    }

    pub(crate) fn radio_selection(&mut self, handle: i32, index: i32) -> i32 {
        let Some(button) = self.buttons.get(&handle).filter(|button| button.kind >= 4) else {
            return -1;
        };
        let Some(root) = button.radio_root else {
            return 1;
        };
        let count = self
            .buttons
            .values()
            .filter(|member| member.radio_root == Some(root))
            .count();
        if index < 0 || index as usize >= count {
            return 2;
        }
        for (member_index, member) in self
            .buttons
            .values_mut()
            .filter(|member| member.radio_root == Some(root))
            .enumerate()
        {
            member.selected = member_index == index as usize;
        }
        0
    }

    fn toggle_button(&mut self, owner: i32, read: &mut BTreeSet<(u8, u16)>) {
        if let Some(key) = self.text_lists.values().find_map(|list| {
            let row = list.rows.iter().position(|&id| id == owner)?;
            list.choice_key(row)
        }) {
            read.insert(key);
        }
        let Some(button) = self
            .buttons
            .get_mut(&owner)
            .filter(|button| button.kind >= 4)
        else {
            return;
        };
        if let Some(root) = button.radio_root {
            for (&handle, member) in &mut self.buttons {
                if member.radio_root == Some(root) {
                    member.selected = handle == owner;
                }
            }
        } else {
            button.selected = !button.selected;
        }
    }

    pub(crate) fn update(
        &mut self,
        input: &Input,
        graphics: &mut Graphics,
        read: &mut BTreeSet<(u8, u16)>,
    ) -> io::Result<i32> {
        let routed = self.routed_buttons();
        let visible =
            |handle: i32, button: &Button| button.enabled && graphics.texture_visible(handle);
        let [x, y] = input.pointer;
        let focus_changed = self.pointer.replace(input.pointer) != Some(input.pointer);
        if focus_changed || input.mouse(0, true) {
            self.focus = routed.iter().copied().find(|&handle| {
                self.hotspots.get(&handle).is_some_and(|hotspot| {
                    let [left, top, width, height] = hotspot.rect;
                    hotspot.enabled
                        && hotspot.mouse
                        && (left..left.saturating_add(width)).contains(&x)
                        && (top..top.saturating_add(height)).contains(&y)
                }) || self.buttons.get(&handle).is_some_and(|button| {
                    visible(handle, button)
                        && graphics.hit_texture(handle, x, y, !button.rectangular_hit)
                })
            });
        }
        let direction = match input.first_action(0, 4, true) {
            0 | 2 => -1,
            1 | 3 => 1,
            _ => 0,
        };
        if direction != 0 {
            let active: Vec<_> = routed
                .iter()
                .copied()
                .filter(|&handle| {
                    self.hotspots
                        .get(&handle)
                        .is_some_and(|hotspot| hotspot.enabled)
                        || self
                            .buttons
                            .get(&handle)
                            .is_some_and(|button| visible(handle, button))
                })
                .collect();
            if !active.is_empty() {
                let index = self
                    .focus
                    .and_then(|handle| active.iter().position(|&id| id == handle));
                self.focus = Some(
                    active[match index {
                        Some(index) => {
                            (index as isize + direction).rem_euclid(active.len() as isize) as usize
                        }
                        None if direction < 0 => active.len() - 1,
                        None => 0,
                    }],
                );
            }
        }
        let mut event = if direction == 0 { 0 } else { 4 };
        if input.mouse(0, true) {
            self.pressed = self.focus;
            if self.pressed.is_some() {
                event = 1;
            }
        } else if !input.mouse(0, false) {
            if let Some(handle) = self.pressed.take().filter(|id| Some(*id) == self.focus) {
                if let Some(command) = self.routed_command(handle, graphics) {
                    self.toggle_button(handle, read);
                    self.command = command;
                    event = 1;
                }
            }
        }
        if input.action(4, true) != 0 {
            if let Some((handle, command)) = self.focus.and_then(|id| {
                self.routed_command(id, graphics)
                    .map(|command| (id, command))
            }) {
                self.toggle_button(handle, read);
                self.command = command;
                event = 2;
            }
        }
        self.refresh(graphics)?;
        Ok(event)
    }

    fn routed_command(&self, handle: i32, graphics: &Graphics) -> Option<i32> {
        if let Some(hotspot) = self.hotspots.get(&handle) {
            return (hotspot.enabled && hotspot.can_activate).then_some(hotspot.command);
        }
        let button = self.buttons.get(&handle)?;
        (button.enabled && graphics.texture_visible(handle)).then_some(button.command)
    }

    fn refresh(&mut self, graphics: &mut Graphics) -> io::Result<()> {
        self.buttons.retain(|&id, button| {
            if graphics.texture_bounds(id).is_some() {
                true
            } else {
                graphics.free_texture(id);
                if button.owns_atlas {
                    graphics.free(button.atlas);
                }
                false
            }
        });
        self.routing.retain(|handle| {
            self.buttons.contains_key(handle)
                || self.text_lists.contains_key(handle)
                || self.hotspots.contains_key(handle)
                || self.scroll_controls.contains_key(handle)
        });
        for (&handle, button) in &mut self.buttons {
            let state =
                button.sprite_frame(self.focus == Some(handle), self.pressed == Some(handle));
            if button.frame != state {
                let left = i32::from(state) * button.width;
                graphics
                    .set_texture_region(handle, [left, 0, left + button.width, button.height])?;
                button.frame = state;
            }
        }
        Ok(())
    }

    fn routed_buttons(&self) -> Vec<i32> {
        let mut handles = Vec::new();
        for &handle in &self.routing {
            if let Some(list) = self.text_lists.get(&handle) {
                handles.extend_from_slice(&list.rows);
            } else {
                handles.push(handle);
            }
        }
        handles
    }

    pub(crate) fn load(
        &mut self,
        assets: &mut GameAssets,
        graphics: &mut Graphics,
        resource: &str,
        first_command: i32,
        priority: u32,
        color_keyed: bool,
    ) -> io::Result<i32> {
        self.clear();
        let target = graphics.display();
        if graphics.surface(target).is_err() {
            return Ok(1);
        }
        let resource = member_key(resource)?;
        let (directory, _) = resource.rsplit_once('/').unwrap_or_default();
        let data = match assets.read(&resource, 16 * 1024 * 1024) {
            Ok(data) => data,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(2),
            Err(error) => return Err(error),
        };
        let layout = Lgm::parse(&data)?;
        let color_key = color_keyed.then_some([0, 255, 0]);
        let mut inserted = Vec::with_capacity(layout.controls.len());
        let mut atlases = Vec::with_capacity(layout.controls.len());
        let background = layout
            .background
            .map(|name| load_image(assets, graphics, directory, name, color_key))
            .transpose()?;
        let result = (|| {
            let mut controls = Vec::with_capacity(layout.controls.len());
            let mut buttons = BTreeMap::new();
            let mut radio_group = None;
            for (index, control) in layout.controls.into_iter().enumerate() {
                let frames = match control.kind {
                    1 | 2 => 3 + u8::from(control.states == 4),
                    3 => 2 + u8::from(control.states == 3),
                    4 => 6 + 2 * u8::from(control.states == 8),
                    5 => 4 + 2 * u8::from(control.states == 8),
                    6 | 8 => 4 + 2 * u8::from(control.states == 6),
                    7 | 9 => 3 + 2 * u8::from(control.states == 5),
                    _ => unreachable!("checked by LGM parser"),
                };
                let atlas = load_image(assets, graphics, directory, control.image, color_key)?;
                atlases.push(atlas);
                let surface = graphics.surface(atlas)?;
                let width = (surface.width / u32::from(frames)) as i32;
                let height = surface.height as i32;
                let texture = graphics.texture_from_surface(
                    atlas,
                    Some([0, 0, width, height]),
                    [i32::from(control.x), i32::from(control.y)],
                )?;
                inserted.push(texture);
                graphics.show_texture(texture, control.visible);
                graphics.texture_opacity(texture, control.opacity);
                if graphics.attach_layer(target, texture, priority, 0, self.overlay, false) != 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid Lucifen menu target",
                    ));
                }
                controls.push(MenuControl {
                    handle: texture,
                    extent: control.extent,
                    states: control.states,
                });
                let radio_root = if (6..=9).contains(&control.kind) {
                    let root = radio_group
                        .filter(|&(kind, _)| kind == control.kind)
                        .map_or(texture, |(_, root)| root);
                    radio_group = Some((control.kind, root));
                    Some(root)
                } else {
                    radio_group = None;
                    None
                };
                buttons.insert(
                    texture,
                    Button {
                        group: self.current_group,
                        atlas,
                        owns_atlas: true,
                        width,
                        height,
                        frames,
                        kind: control.kind,
                        frame: 0,
                        enabled: true,
                        rectangular_hit: color_keyed,
                        selected: radio_root == Some(texture),
                        radio_root,
                        command: if first_command < 0 {
                            -1
                        } else {
                            first_command.saturating_add(index as i32)
                        },
                        tooltip: Vec::new(),
                    },
                );
            }
            if let Some(background) = background {
                graphics.set_background_image(target, background, [0, 0], false)?;
            }
            Ok::<_, io::Error>((controls, buttons))
        })();
        if let Some(background) = background {
            graphics.free(background);
        }
        match result {
            Ok((controls, buttons)) => {
                self.routing
                    .extend(controls.iter().map(|control| control.handle));
                self.controls = controls;
                self.buttons.extend(buttons);
                Ok(0)
            }
            Err(error) => {
                for texture in inserted {
                    graphics.free_texture(texture);
                }
                for atlas in atlases {
                    graphics.free(atlas);
                }
                Err(error)
            }
        }
    }
}

impl TextList {
    fn choice_key(&self, row: usize) -> Option<(u8, u16)> {
        let (group, base) = self.annotation_key?;
        Some((group, base.wrapping_add(row as u16)))
    }

    pub(crate) fn next_choice_key(&self) -> Option<(u8, u16)> {
        self.choice_key(self.rows.len())
    }

    fn position_rows(&self, graphics: &mut Graphics) {
        let [_, height, gap] = self.layout;
        let span = i64::from(height) + i64::from(gap);
        let total = i64::from(height) + span * self.rows.len().saturating_sub(1) as i64;
        let top = i64::from(self.position[1]) - total / 2;
        for (index, &texture) in self.rows.iter().enumerate() {
            let y = (top + span * index as i64).clamp(i64::from(i32::MIN), i64::from(i32::MAX));
            graphics.texture_move(texture, self.position[0], y as i32, false);
        }
    }
}

fn load_image(
    assets: &mut GameAssets,
    graphics: &mut Graphics,
    directory: &str,
    name: &[u8],
    color_key: Option<[u8; 3]>,
) -> io::Result<i32> {
    let name = encoding_rs::SHIFT_JIS
        .decode_without_bom_handling_and_without_replacement(name)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid Lucifen menu image name",
            )
        })?;
    let bytes = assets.read(&format!("{directory}/{name}"), 64 * 1024 * 1024)?;
    graphics.load_image(&bytes, color_key)
}
