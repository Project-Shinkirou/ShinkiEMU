use super::texture::{frame_from_image, validate_dimensions};
use super::*;
use crate::MusicaStageCommand;

impl Scene {
    pub(super) fn stage(
        &mut self,
        commands: &mut Vec<SceneCommand>,
        stage: &MusicaStageCommand,
        opacity: f32,
    ) -> SessionResult<()> {
        if let Some(background) = &stage.background {
            // The native base background copies RGB without source-alpha
            // weighting. While a transition fades the new stage in, the layer
            // must composite over the previous frame instead of replacing it.
            let blend = if opacity < 1.0 {
                BlendMode::Alpha
            } else {
                BlendMode::Opaque
            };
            self.layer_with_blend(
                commands,
                &background.resource_uri,
                [background.x, background.y],
                opacity,
                blend,
            )?;
        }
        for stand in &stage.stands {
            self.stand(commands, stand, opacity)?;
        }
        // Native foreground order (sub_42E900): the main slot and further comma
        // slots share the reference position; the colon prefix and the `+` slot
        // draw after the main foreground at the stage origin.
        let reference = stage.reference_position.unwrap_or([0, 0]);
        self.stage_foreground(
            commands,
            stage.foreground.main.as_deref(),
            reference,
            opacity,
        )?;
        self.stage_foreground(
            commands,
            stage.foreground.second.as_deref(),
            [0, 0],
            opacity,
        )?;
        self.stage_foreground(commands, stage.foreground.third.as_deref(), [0, 0], opacity)?;
        self.stage_foreground(
            commands,
            stage.foreground.prefix.as_deref(),
            [0, 0],
            opacity,
        )?;
        self.stage_foreground(commands, stage.foreground.plus.as_deref(), [0, 0], opacity)?;
        self.draw_ani_patches(commands, stage, opacity)?;
        Ok(())
    }

    fn stage_foreground(
        &mut self,
        commands: &mut Vec<SceneCommand>,
        uri: Option<&str>,
        position: [i32; 2],
        opacity: f32,
    ) -> SessionResult<()> {
        let Some(uri) = uri else {
            return Ok(());
        };
        let blend = foreground_blend(uri)?;
        self.layer_with_blend(commands, uri, position, opacity, blend)
    }
}

impl Scene {
    /// Draws a stage through a transition rule mask: every layer becomes a
    /// `MaskReveal` over the fully drawn previous stage, matching the native
    /// masked transition for opaque stacks (the observed game usage).
    pub(super) fn stage_masked(
        &mut self,
        commands: &mut Vec<SceneCommand>,
        stage: &MusicaStageCommand,
        mask: &TextureFrame,
        mode: MaskRevealMode,
        phase: f32,
    ) -> SessionResult<()> {
        if let Some(background) = &stage.background {
            self.masked_layer(
                commands,
                &background.resource_uri,
                [background.x, background.y],
                mask,
                mode,
                phase,
            )?;
        }
        for stand in &stage.stands {
            self.masked_stand(commands, stand, mask, mode, phase)?;
        }
        let reference = stage.reference_position.unwrap_or([0, 0]);
        for (uri, position) in [
            (stage.foreground.main.as_deref(), reference),
            (stage.foreground.second.as_deref(), [0, 0]),
            (stage.foreground.third.as_deref(), [0, 0]),
            (stage.foreground.prefix.as_deref(), [0, 0]),
            (stage.foreground.plus.as_deref(), [0, 0]),
        ] {
            if let Some(uri) = uri {
                self.masked_layer(commands, uri, position, mask, mode, phase)?;
            }
        }
        Ok(())
    }

    fn masked_layer(
        &mut self,
        commands: &mut Vec<SceneCommand>,
        uri: &str,
        [x, y]: [i32; 2],
        mask: &TextureFrame,
        mode: MaskRevealMode,
        phase: f32,
    ) -> SessionResult<()> {
        let asset = self.texture_asset_animated(uri)?;
        let x = x.checked_add(asset.logical_origin[0]).ok_or_else(|| {
            error(
                "SHINKI_EMU_MUSICA_LAYER_POSITION",
                "layer x position overflows",
            )
        })?;
        let y = y.checked_add(asset.logical_origin[1]).ok_or_else(|| {
            error(
                "SHINKI_EMU_MUSICA_LAYER_POSITION",
                "layer y position overflows",
            )
        })?;
        commands.push(SceneCommand::MaskReveal {
            id: format!("masked-layer:{}", commands.len()),
            frame: asset.frame,
            mask: mask.clone(),
            destination: RectI {
                x,
                y,
                width: asset.logical_extent.width,
                height: asset.logical_extent.height,
            },
            mode,
            phase,
            opacity: 1.0,
        });
        Ok(())
    }
}

impl Scene {
    pub(crate) fn ani_source(
        &mut self,
        ani_uri: &str,
    ) -> SessionResult<Arc<crate::MusicaAniArchive>> {
        if let Some(archive) = self.ani_sources.get(ani_uri) {
            return Ok(archive.clone());
        }
        let bytes = read_asset(&self.archive, ani_uri, MAX_ASSET_BYTES)?;
        let archive = Arc::new(
            crate::MusicaAniArchive::parse(Arc::<[u8]>::from(bytes.as_ref()))
                .map_err(core_error)?,
        );
        self.ani_sources.put(ani_uri.to_owned(), archive.clone());
        Ok(archive)
    }

    /// Draws the current eye/mouth patch frames of every attached ANI patch
    /// set over its stage layer, matching the native `<char> <state>` row
    /// selection and per-frame offsets.
    pub(super) fn draw_ani_patches(
        &mut self,
        commands: &mut Vec<SceneCommand>,
        stage: &MusicaStageCommand,
        opacity: f32,
    ) -> SessionResult<()> {
        if self.frame_ani_patches.is_empty() {
            return Ok(());
        }
        const EYE_WORDS: [&str; 3] = ["開き目", "中目", "閉じ目"];
        const MOUTH_WORDS: [&str; 3] = ["閉じ口", "中口", "開き口"];
        let mut candidates: Vec<&str> = stage.foreground.uris_in_order();
        candidates.extend(
            stage
                .background
                .as_ref()
                .map(|layer| layer.resource_uri.as_str()),
        );
        for uri in candidates {
            let Some(patch) = self.frame_ani_patches.get(uri) else {
                continue;
            };
            let characters = patch
                .characters
                .iter()
                .map(|character| {
                    (
                        character.name.clone(),
                        character.eye_state,
                        character.mouth_state,
                    )
                })
                .collect::<Vec<_>>();
            for (name, eye, mouth) in characters {
                let mut labels = vec![format!("{name} {}", EYE_WORDS[eye as usize])];
                if mouth != 0 {
                    labels.push(format!("{name} {}", MOUTH_WORDS[mouth as usize]));
                }
                for label in labels {
                    self.push_ani_patch(commands, uri, &label, opacity)?;
                }
            }
        }
        Ok(())
    }

    fn push_ani_patch(
        &mut self,
        commands: &mut Vec<SceneCommand>,
        png_uri: &str,
        label: &str,
        opacity: f32,
    ) -> SessionResult<()> {
        let Some((stem, _)) = png_uri.rsplit_once('.') else {
            return Ok(());
        };
        let ani_uri = format!("{stem}.ani");
        let archive = self.ani_source(&ani_uri)?;
        let Some(index) = archive
            .frames()
            .iter()
            .position(|frame| frame.name == label)
        else {
            return Ok(());
        };
        let frame = archive.frames().get(index).ok_or_else(|| {
            error(
                "SHINKI_EMU_MUSICA_ANI_PATCH_FRAME",
                "ANI patch frame index is invalid",
            )
        })?;
        let destination = RectI {
            x: i32::from(frame.offset_x),
            y: i32::from(frame.offset_y),
            width: frame.width,
            height: frame.height,
        };
        let cache_key = format!("native:{ani_uri}#{index}");
        if self.textures.get(&cache_key).is_none() {
            validate_dimensions(frame.width, frame.height)?;
            let decoded = frame_from_image(archive.decode_frame(index).map_err(core_error)?);
            self.textures
                .insert(cache_key.clone(), decoded)
                .map_err(|_| {
                    error(
                        "SHINKI_EMU_MUSICA_IMAGE_BOUND",
                        "decoded image exceeds the texture cache budget",
                    )
                })?;
        }
        commands.push(SceneCommand::Texture {
            id: format!("ani-patch:{}", commands.len()),
            frame: self.textures.get(&cache_key).ok_or_else(|| {
                error(
                    "SHINKI_EMU_MUSICA_ANI_PATCH_TEXTURE",
                    "ANI patch texture is missing from the cache",
                )
            })?,
            destination,
            opacity,
            blend: BlendMode::Alpha,
        });
        Ok(())
    }
}

fn foreground_blend(uri: &str) -> SessionResult<BlendMode> {
    let suffix = uri.get(uri.len().saturating_sub(7)..).unwrap_or("");
    if suffix.eq_ignore_ascii_case("_sc.png") {
        Ok(BlendMode::Screen)
    } else if suffix.eq_ignore_ascii_case("_ov.png") {
        Err(error(
            "SHINKI_EMU_MUSICA_STAGE_BLEND",
            "native overlay compositing is not implemented",
        ))
    } else {
        Ok(BlendMode::Alpha)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_suffix_selects_screen_and_rejects_overlay() {
        assert_eq!(
            foreground_blend("musica:/bg/light_SC.PNG").unwrap(),
            BlendMode::Screen
        );
        assert_eq!(
            foreground_blend("musica:/bg/light.png").unwrap(),
            BlendMode::Alpha
        );
        assert_eq!(
            foreground_blend("musica:/bg/light_ov.png")
                .unwrap_err()
                .code(),
            "SHINKI_EMU_MUSICA_STAGE_BLEND"
        );
    }
}
