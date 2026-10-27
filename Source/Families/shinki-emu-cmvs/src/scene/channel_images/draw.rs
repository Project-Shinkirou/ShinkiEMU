use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn append_channel_layer(
    scene: &CmvsScene,
    vm: &CmvsPs2aVmState,
    index: u8,
    child: u32,
    root: &crate::vm::CmvsEffectElement,
    element: &crate::vm::CmvsEffectElement,
    frame: &TextureFrame,
    texture_id: String,
    commands: &mut Vec<SceneCommand>,
    phase: u32,
    crossfade: Option<(String, f32)>,
) -> Result<(), CoreError> {
    let has_alpha = if element.image_resource.is_some() {
        element.image_info[2] != 0
    } else {
        vm.effect_channels[&index].image_has_alpha
    };
    append_channel_layer_size(
        scene,
        vm,
        index,
        child,
        root,
        element,
        [frame.width, frame.height],
        texture_id,
        commands,
        phase,
        crossfade,
        has_alpha,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn append_channel_layer_size(
    scene: &CmvsScene,
    vm: &CmvsPs2aVmState,
    index: u8,
    child: u32,
    root: &crate::vm::CmvsEffectElement,
    element: &crate::vm::CmvsEffectElement,
    size: [u32; 2],
    texture_id: String,
    commands: &mut Vec<SceneCommand>,
    phase: u32,
    crossfade: Option<(String, f32)>,
    has_alpha: bool,
) -> Result<(), CoreError> {
    if element.visible == 0
        || element.alpha <= 0
        || phase == 0
        || (child != u32::MAX && !matches!(element.basic_kind, 2 | 3) && element.order < 0)
        || (child != u32::MAX && element.basic_kind != root.basic_kind)
    {
        return Ok(());
    }
    let Some(source) = element.source else {
        return Ok(());
    };
    let stage_width = scene
        .logical_extent()
        .map_or(vm.native_stage_size[0], |extent| extent.width);
    let inherits = child != u32::MAX && element.inheritance_flags & 1 != 0;
    let (x, y) = vm.effect_channels[&index].origin();
    let root_origin = root.position.unwrap_or([x, y]);
    let origin = if child == u32::MAX || inherits {
        root_origin
    } else {
        [0; 2]
    };
    let position = if child == u32::MAX {
        [0; 2]
    } else {
        element.position.unwrap_or([0; 2])
    };
    let Some((source, destination)) = clipped_region(source, position, origin, size)? else {
        return Ok(());
    };
    let alpha = if inherits {
        element.alpha.clamp(0, 255) * root.alpha.clamp(0, 255) / 255
    } else {
        element.alpha.clamp(0, 255)
    };
    if alpha == 0 {
        return Ok(());
    }
    let mode = if phase != 256 {
        1
    } else if !has_alpha && alpha >= 255 {
        0
    } else {
        element.blend_mode
    };
    let (blend, opacity) = match mode {
        0 => (BlendMode::Opaque, 1.0),
        2 => (BlendMode::Add, alpha as f32 / 255.0),
        3 => (
            BlendMode::ScreenRgb,
            element.alpha.clamp(0, 255) as f32 / 255.0,
        ),
        _ => (BlendMode::Alpha, alpha as f32 / 255.0),
    };
    let opacity = opacity * phase as f32 / 256.0;
    // Composite replacement draws old, overlap and current in one submission.
    let id = format!("cmvs.channel.{index}.{child}.{texture_id}");
    let command = match crossfade {
        Some((previous_texture_id, progress)) => SceneCommand::CrossfadeSprite {
            id,
            previous_texture_id,
            current_texture_id: texture_id,
            source: Some(source),
            destination,
            progress,
            opacity,
        },
        None => SceneCommand::Sprite {
            id,
            texture_id,
            source: Some(source),
            destination,
            opacity,
            blend,
        },
    };
    let shake = if root.inheritance_flags & 0x100 != 0 {
        crate::vm::screen_shake::logical_offset(vm, stage_width, 0)?
    } else {
        [0.0; 2]
    };
    if matches!(element.basic_kind, 2 | 3) {
        let mut parent = super::super::native_save_channels::element_geometry(root);
        parent.auxiliary_pair_words = Some(root_origin.map(|word| word as u32));
        let local = super::super::native_save_channels::element_child_geometry(element);
        let child_context = (child != u32::MAX).then_some(&local);
        if let Some(geometry) =
            surface::geometry_with_surfaces(vm, &parent, child_context, stage_width, true)
        {
            geometry
                .with_offset(shake)
                .append(commands, command, Some(size))?;
        }
        return Ok(());
    }
    let reference = if inherits { root.width } else { element.width } as u32;
    if reference != 0 && stage_width == 0 {
        return Err(error("SHINKI_EMU_CMVS_DISPLAY_UNBOUND"));
    }
    let basis = projection::reference_scale(stage_width, reference);
    let parent_scale = root.scale_words.map(f32::from_bits);
    let local = element.scale_words.map(f32::from_bits);
    let scale = if inherits {
        [parent_scale[0] * local[0], parent_scale[1] * local[1]]
    } else {
        local
    };
    if scale
        .iter()
        .any(|scale| !scale.is_finite() || *scale == 0.0)
    {
        return Ok(());
    }
    let inherited_position = if inherits { parent_scale[0] } else { 1.0 };
    let pivot = element.pivot.map(|word| word as i16 as f32);
    let transform = roukaku_media_core::Transform2D {
        m11: basis * scale[0],
        m22: basis * scale[1],
        tx: basis
            * (origin[0] as f32 + position[0] as f32 * inherited_position
                - (origin[0] as f32 + position[0] as f32 + pivot[0]) * scale[0])
            + shake[0],
        ty: basis
            * (origin[1] as f32 + position[1] as f32 * inherited_position
                - (origin[1] as f32 + position[1] as f32 + pivot[1]) * scale[1])
            + shake[1],
        ..roukaku_media_core::Transform2D::IDENTITY
    };
    surface::extend_transformed(commands, transform, [command]);
    Ok(())
}
