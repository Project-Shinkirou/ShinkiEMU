use super::*;
impl Scene {
    pub fn render_title(
        &mut self,
        title: &crate::runtime::title::TitleVariant,
        focus: Option<u32>,
    ) -> SessionResult<()> {
        let mut commands = vec![SceneCommand::Clear {
            rgba: [0, 0, 0, 255],
        }];
        self.layer(&mut commands, &title.background, 0, 0, 1.0)?;
        if let (Some(focus), Some(hover)) = (focus, &title.hover) {
            let button = title.buttons.get(focus as usize).ok_or_else(|| {
                error(
                    "SHINKI_EMU_MUSICA_TITLE_FOCUS",
                    "title focus is outside the menu",
                )
            })?;
            let [x, y, width, height] = button.rect;
            commands.push(SceneCommand::PushClip {
                rect: RectI {
                    x,
                    y,
                    width: width as u32,
                    height: height as u32,
                },
            });
            self.layer(&mut commands, hover, 0, 0, 1.0)?;
            commands.push(SceneCommand::PopClip);
        }
        self.submit(commands)
    }
}
