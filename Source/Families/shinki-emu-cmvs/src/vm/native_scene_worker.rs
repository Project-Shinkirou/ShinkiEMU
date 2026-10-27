use super::native_hit_polygon::SavedNativeCollisionParticles;
use super::native_mesh::SavedNativeSnow;
use super::native_particle_kind1::SavedNativeParticleKind1;
use super::native_particle_kind3::SavedNativeParticleKind3;
use super::native_particle_kind4::SavedNativeParticleKind4;
use super::native_particle_kind5::SavedNativeParticleKind5;
use super::native_particle_kind7::SavedNativeParticleKind7;
use super::native_particle_kind9::SavedNativeParticleKind9;
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsNativeSceneWorker {
    Kind1(CmvsNativeParticleKind1),
    MeshSnow(CmvsNativeSnow),
    Kind3(CmvsNativeParticleKind3),
    Kind4(CmvsNativeParticleKind4),
    Kind5(CmvsNativeParticleKind5),
    CollisionParticles(CmvsNativeCollisionParticles),
    Kind7(CmvsNativeParticleKind7),
    Kind9(CmvsNativeParticleKind9),
    Empty,
    ImageShards(Box<CmvsNativeImageShards>),
    Scene50(CmvsNativeSceneKind50),
    Scene51(CmvsNativeSceneKind51),
    Scene52(Box<CmvsNativeSceneKind52>),
    Scene53(CmvsNativeSceneKind53),
    Scene54(Box<CmvsNativeSceneKind54>),
    Scene55(CmvsNativeSceneKind55),
    Scene56(Box<CmvsNativeSceneKind56>),
    Scene57(CmvsNativeSceneKind57),
}
impl CmvsNativeSceneWorker {
    pub(crate) fn basic_resource_count(&self) -> usize {
        match self {
            Self::Scene53(_) => 1,
            Self::Scene55(owner) => owner.resource_count(),
            Self::Kind1(_)
            | Self::Kind3(_)
            | Self::Kind4(_)
            | Self::Kind5(_)
            | Self::CollisionParticles(_)
            | Self::Kind7(_)
            | Self::Kind9(_) => 1,
            _ => 0,
        }
    }
    pub(crate) fn basic_resource(&self, index: usize) -> Option<&[u8]> {
        if let Self::Scene53(owner) = self {
            return (index == 0).then(|| owner.resource(2)).flatten();
        }
        if let Self::Scene55(owner) = self {
            return owner.resource(index);
        }
        if index != 0 {
            return None;
        }
        Some(match self {
            Self::Kind1(owner) => &owner.resource,
            Self::Kind3(owner) => &owner.resource,
            Self::Kind4(owner) => &owner.resource,
            Self::Kind5(owner) => &owner.resource,
            Self::CollisionParticles(owner) => &owner.resource,
            Self::Kind7(owner) => &owner.resource,
            Self::Kind9(owner) => &owner.resource,
            _ => return None,
        })
    }
    pub(crate) fn basic_resource_index(&self, index: usize) -> Option<usize> {
        match self {
            Self::Scene55(owner) if index < owner.particle_capacity() => {
                Some(owner.basic_resource_index(index))
            }
            Self::Scene55(_) => None,
            _ if self.basic_resource_count() != 0 => Some(0),
            _ => None,
        }
    }
    pub(crate) fn image_shards(&self) -> Option<&CmvsNativeImageShards> {
        match self {
            Self::ImageShards(image) => Some(image),
            _ => None,
        }
    }
    pub(crate) fn status(&self) -> Result<u32, CoreError> {
        if let Self::Scene56(owner) = self {
            return owner.status();
        }
        Ok(match self {
            Self::ImageShards(image) => image.grid.status() as u32,
            Self::Scene51(owner) => owner.status(),
            Self::Scene52(owner) => owner.status(),
            Self::Scene53(owner) => owner.status(),
            Self::Scene54(owner) => owner.status(),
            Self::Scene55(owner) => owner.status(),
            Self::Scene57(owner) => owner.status(),
            _ => u32::MAX,
        })
    }
    pub(crate) fn mesh_count(&self) -> usize {
        match self {
            Self::MeshSnow(owner) => owner.particles.len(),
            Self::Scene50(owner) => owner.mesh_count(),
            Self::Scene52(owner) => owner.mesh_count(),
            Self::Scene53(owner) => owner.mesh_count(),
            Self::Scene54(owner) => owner.mesh_count(),
            Self::Scene56(owner) => owner.mesh_count(),
            Self::Scene57(owner) => owner.mesh_count(),
            _ => 0,
        }
    }
    pub(crate) fn mesh(&self, index: usize) -> Option<&CmvsNativeMesh> {
        match self {
            Self::MeshSnow(owner) => owner.particles.get(index).map(|particle| &particle.mesh),
            Self::Scene50(owner) => owner.mesh(index),
            Self::Scene52(owner) => owner.mesh(index),
            Self::Scene53(owner) => owner.mesh(index),
            Self::Scene54(owner) => owner.mesh(index),
            Self::Scene56(owner) => owner.mesh(index),
            Self::Scene57(owner) => owner.mesh(index),
            _ => None,
        }
    }
    pub(crate) fn mesh_queue_count(&self) -> usize {
        match self {
            Self::Scene53(owner) => (0..owner.mesh_count())
                .filter(|&index| owner.mesh_visible(index))
                .count(),
            _ => self.mesh_count(),
        }
    }
    pub(crate) fn mesh_visible(&self, index: usize) -> bool {
        match self {
            Self::Scene53(owner) => owner.mesh_visible(index),
            _ => true,
        }
    }
    pub(crate) fn prepare_draw(&mut self, delta_ms: u32) -> Result<(), CoreError> {
        if let Self::Scene53(owner) = self {
            owner.prepare_draw(delta_ms)?;
        }
        Ok(())
    }
    pub(crate) fn mesh_resource_count(&self) -> usize {
        match self {
            Self::MeshSnow(_) => 1,
            Self::Scene50(_) => 2,
            Self::Scene53(_) => 2,
            Self::Scene52(owner) => owner.resource_count(),
            Self::Scene54(owner) => owner.resource_count(),
            Self::Scene56(owner) => owner.resource_count(),
            Self::Scene57(owner) => owner.resource_count(),
            _ => 0,
        }
    }
    pub(crate) fn mesh_resource(&self, index: usize) -> Option<&[u8]> {
        match self {
            Self::MeshSnow(owner) if index == 0 => Some(&owner.resource),
            Self::Scene50(owner) => owner.resources.get(index).map(Vec::as_slice),
            Self::Scene52(owner) => owner.resource(index),
            Self::Scene53(owner) if index < 2 => owner.resource(index),
            Self::Scene54(owner) => owner.resource(index),
            Self::Scene56(owner) => owner.resource(index),
            Self::Scene57(owner) => owner.resource(index),
            _ => None,
        }
    }
    pub(crate) fn mesh_resource_index(&self, index: usize) -> Option<usize> {
        match self {
            Self::MeshSnow(owner) if index < owner.particles.len() => Some(0),
            Self::Scene50(owner) if index < owner.mesh_count() => {
                Some(owner.mesh_resource_index(index))
            }
            Self::Scene52(owner) if index < owner.mesh_count() => {
                Some(owner.mesh_resource_index(index))
            }
            Self::Scene53(owner) if index < owner.mesh_count() => {
                Some(owner.mesh_resource_index(index))
            }
            Self::Scene54(owner) if index < owner.mesh_count() => {
                Some(owner.mesh_resource_index(index))
            }
            Self::Scene56(owner) if index < owner.mesh_count() => {
                Some(owner.mesh_resource_index(index))
            }
            Self::Scene57(owner) if index < owner.mesh_count() => {
                Some(owner.mesh_resource_index(index))
            }
            _ => None,
        }
    }
    pub(crate) fn mesh_resource_revision(&self, index: usize) -> u32 {
        match self {
            Self::Scene54(owner) if index == 5 => owner.resource_reload_generation(),
            Self::Scene57(owner) if matches!(index, 2 | 3) => owner.resource_reload_generation(),
            _ => 0,
        }
    }
    pub(crate) fn particle_count(&self) -> usize {
        match self {
            Self::Kind1(owner) => owner.draw_indices().len(),
            Self::Kind3(owner) => owner.particle_capacity(),
            Self::Kind4(owner) => owner.draw_indices().len(),
            Self::Kind5(owner) => owner.particle_capacity(),
            Self::CollisionParticles(owner) => owner.particles.len(),
            Self::Kind7(owner) => owner.particle_capacity(),
            Self::Kind9(owner) => owner.particle_capacity(),
            Self::Scene53(_) => 1,
            Self::Scene55(owner) => owner.particle_capacity(),
            Self::MeshSnow(_)
            | Self::ImageShards(_)
            | Self::Scene50(_)
            | Self::Scene51(_)
            | Self::Scene52(_)
            | Self::Scene54(_)
            | Self::Scene56(_)
            | Self::Scene57(_)
            | Self::Empty => 0,
        }
    }
    pub(crate) fn particle_index(&self, ordinal: usize) -> usize {
        match self {
            Self::Kind1(owner) => usize::from(owner.draw_indices()[ordinal]),
            Self::Kind4(owner) => usize::from(owner.draw_indices()[ordinal]),
            _ => ordinal,
        }
    }
    pub(crate) fn plane_position(&self, index: usize) -> Option<[u32; 3]> {
        match self {
            Self::Kind1(owner) => owner.plane_position(index),
            Self::Kind3(owner) => owner.plane_position(index),
            Self::Kind4(owner) => owner.plane_position(index),
            Self::Kind5(owner) => owner.plane_position(index),
            Self::Kind7(owner) => owner.plane_position(index),
            Self::Scene55(owner) => owner.plane_position(index),
            Self::CollisionParticles(owner) => owner
                .particles
                .get(index)
                .map(|particle| particle.position_words),
            Self::MeshSnow(_)
            | Self::Kind9(_)
            | Self::ImageShards(_)
            | Self::Scene50(_)
            | Self::Scene51(_)
            | Self::Scene52(_)
            | Self::Scene53(_)
            | Self::Scene54(_)
            | Self::Scene56(_)
            | Self::Scene57(_)
            | Self::Empty => None,
        }
    }
    pub(crate) fn particle_basic(&self, index: usize) -> Option<CmvsTextureParentState> {
        match self {
            Self::Kind1(owner) => owner.draw_basic(index),
            Self::Kind3(owner) => owner.draw_basic(index),
            Self::Kind4(owner) => owner.draw_basic(index),
            Self::Kind5(owner) => owner.draw_basic(index),
            Self::Kind7(owner) => owner.draw_basic(index),
            Self::Kind9(owner) => owner.draw_basic(index),
            Self::Scene53(owner) if index == 0 => Some(owner.backdrop_basic()),
            Self::Scene53(_) => None,
            Self::Scene55(owner) => owner.draw_basic(index),
            Self::CollisionParticles(owner) => {
                owner
                    .particles
                    .get(index)
                    .map(|particle| CmvsTextureParentState {
                        surface_initialized: true,
                        basic_kind: 2,
                        alpha: Some(particle.alpha),
                        rect_words: Some([0, 0, 32, 32]),
                        position_words: Some([16, 16]),
                        float_position_words: particle.position_words,
                        projection_height_word: owner.projection_height_word(),
                        screen_shake_disabled: true,
                        ..Default::default()
                    })
            }
            Self::MeshSnow(_)
            | Self::ImageShards(_)
            | Self::Scene50(_)
            | Self::Scene51(_)
            | Self::Scene52(_)
            | Self::Scene54(_)
            | Self::Scene56(_)
            | Self::Scene57(_)
            | Self::Empty => None,
        }
    }
    pub(crate) fn basic_queue_depth(&self, index: usize, camera_z: f32) -> Option<(u8, f32)> {
        if matches!(self, Self::Scene53(_)) {
            return (index == 0).then_some((3, 64.0));
        }
        if let Self::Kind9(owner) = self {
            return owner
                .draw_order(index)
                .filter(|order| (*order as i32) >= 0)
                .map(|order| (3, order as i32 as f32));
        }
        let depth = f32::from_bits(self.plane_position(index)?[2]) - camera_z;
        (depth > 0.0).then_some((1, depth))
    }
    pub(crate) fn sound_resources(&self) -> &'static [(u8, &'static [u8])] {
        match self {
            Self::Scene51(_) => &CmvsNativeSceneKind51::SOUND_RESOURCES,
            Self::Scene52(_) => &CmvsNativeSceneKind52::SOUND_RESOURCES,
            Self::Scene54(_) => &CmvsNativeSceneKind54::SOUND_RESOURCES,
            Self::Scene55(_) => &CmvsNativeSceneKind55::SOUND_RESOURCES,
            _ => &[],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SavedNativeSceneWorker {
    Kind1(SavedNativeParticleKind1),
    MeshSnow(SavedNativeSnow),
    Kind3(SavedNativeParticleKind3),
    Kind4(SavedNativeParticleKind4),
    Kind5(SavedNativeParticleKind5),
    CollisionParticles(SavedNativeCollisionParticles),
    Kind7(SavedNativeParticleKind7),
    Kind9(SavedNativeParticleKind9),
    Marker(u16),
}
impl SavedNativeSceneWorker {
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, CoreError> {
        match bytes.get(..4) {
            Some([1, 0, 1, 0]) => SavedNativeParticleKind1::decode(bytes).map(Self::Kind1),
            Some([1, 0, 2, 0]) => SavedNativeSnow::decode(bytes).map(Self::MeshSnow),
            Some([1, 0, 3, 0]) => SavedNativeParticleKind3::decode(bytes).map(Self::Kind3),
            Some([1, 0, 4, 0]) => SavedNativeParticleKind4::decode(bytes).map(Self::Kind4),
            Some([1, 0, 5, 0]) => SavedNativeParticleKind5::decode(bytes).map(Self::Kind5),
            Some([1, 0, 6, 0]) => {
                SavedNativeCollisionParticles::decode(bytes).map(Self::CollisionParticles)
            }
            Some([1, 0, 7, 0]) => SavedNativeParticleKind7::decode(bytes).map(Self::Kind7),
            Some([1, 0, 9, 0]) => SavedNativeParticleKind9::decode(bytes).map(Self::Kind9),
            Some([1, 0, kind, 0]) if bytes.len() == 4 && matches!(*kind, 0 | 8 | 0x50..=0x57) => {
                Ok(Self::Marker(u16::from(*kind)))
            }
            _ => Err(invalid("unsupported native scene worker record")),
        }
    }
    pub(crate) fn encode(&self) -> Result<Vec<u8>, CoreError> {
        match self {
            Self::Kind1(value) => value.encode(),
            Self::MeshSnow(value) => value.encode(),
            Self::Kind3(value) => value.encode(),
            Self::Kind4(value) => value.encode(),
            Self::Kind5(value) => value.encode(),
            Self::CollisionParticles(value) => value.encode(),
            Self::Kind7(value) => value.encode(),
            Self::Kind9(value) => value.encode(),
            Self::Marker(kind) if matches!(*kind, 0 | 8 | 0x50..=0x57) => {
                Ok(vec![1, 0, *kind as u8, 0])
            }
            Self::Marker(_) => Err(invalid("unsupported native scene marker kind")),
        }
    }
}
fn invalid(message: &'static str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_NATIVE_SCENE_WORKER", message)
}
pub(crate) fn validate(vm: &CmvsPs2aVmState) -> Result<(), CoreError> {
    for (&slot, worker) in &vm.native_scene_workers {
        if slot > 3 {
            return Err(invalid("native scene worker slot exceeds three"));
        }
        match worker {
            CmvsNativeSceneWorker::Kind1(owner) => owner.validate()?,
            CmvsNativeSceneWorker::MeshSnow(owner) => owner.validate()?,
            CmvsNativeSceneWorker::Kind3(owner) => owner.validate()?,
            CmvsNativeSceneWorker::Kind4(owner) => owner.validate()?,
            CmvsNativeSceneWorker::Kind5(owner) => owner.validate()?,
            CmvsNativeSceneWorker::CollisionParticles(owner) => owner.validate()?,
            CmvsNativeSceneWorker::Kind7(owner) => owner.validate()?,
            CmvsNativeSceneWorker::Kind9(owner) => owner.validate()?,
            CmvsNativeSceneWorker::ImageShards(owner) => owner.validate()?,
            CmvsNativeSceneWorker::Scene50(owner) => owner.validate()?,
            CmvsNativeSceneWorker::Scene51(owner) => owner.validate()?,
            CmvsNativeSceneWorker::Scene52(owner) => owner.validate()?,
            CmvsNativeSceneWorker::Scene53(owner) => owner.validate()?,
            CmvsNativeSceneWorker::Scene54(owner) => owner.validate()?,
            CmvsNativeSceneWorker::Scene55(owner) => owner.validate()?,
            CmvsNativeSceneWorker::Scene56(owner) => owner.validate()?,
            CmvsNativeSceneWorker::Scene57(owner) => owner.validate()?,
            CmvsNativeSceneWorker::Empty => {}
        }
    }
    Ok(())
}
pub(crate) fn start_scene50(
    vm: &mut CmvsPs2aVmState,
    slot: u32,
    resources: [Vec<u8>; 2],
) -> Result<bool, CoreError> {
    if slot > 3 {
        return Ok(false);
    }
    let owner = CmvsNativeSceneKind50::new(resources)?;
    remove_slot(vm, slot);
    vm.native_scene_workers
        .insert(slot as u8, CmvsNativeSceneWorker::Scene50(owner));
    vm.write_interpreter_word(2948 + 4 * slot, 1);
    Ok(true)
}
pub(crate) fn install_scene53(
    vm: &mut CmvsPs2aVmState,
    slot: u8,
    owner: CmvsNativeSceneKind53,
) -> Result<(), CoreError> {
    if slot > 3 {
        return Err(invalid("native scene worker slot exceeds three"));
    }
    owner.validate()?;
    let frame = owner.initial_frame();
    remove_slot(vm, u32::from(slot));
    apply_frame(&mut vm.independent_views[0], frame);
    vm.native_scene_workers
        .insert(slot, CmvsNativeSceneWorker::Scene53(owner));
    vm.write_interpreter_word(2948 + 4 * u32::from(slot), 1);
    Ok(())
}
pub(crate) fn start_controller(
    vm: &mut CmvsPs2aVmState,
    command: u16,
    slot: u32,
) -> Result<bool, CoreError> {
    if slot > 3 {
        return Ok(false);
    }
    let mut rng = vm.prng_state;
    let (worker, frame) = match command {
        657 => {
            let owner = CmvsNativeSceneKind51::new(vm.session_clock_millis);
            let frame = owner.initial_frame();
            (CmvsNativeSceneWorker::Scene51(owner), frame)
        }
        658 => {
            let owner = CmvsNativeSceneKind52::new()?;
            let frame = owner.initial_frame();
            (CmvsNativeSceneWorker::Scene52(Box::new(owner)), frame)
        }
        660 => {
            let owner = CmvsNativeSceneKind54::new(&mut rng)?;
            let frame = owner.initial_frame();
            (CmvsNativeSceneWorker::Scene54(Box::new(owner)), frame)
        }
        661 => {
            let owner = CmvsNativeSceneKind55::new()?;
            let frame = owner.initial_frame();
            (CmvsNativeSceneWorker::Scene55(owner), frame)
        }
        662 => {
            let owner = CmvsNativeSceneKind56::new()?;
            let frame = owner.initial_frame();
            (CmvsNativeSceneWorker::Scene56(Box::new(owner)), frame)
        }
        663 => {
            let owner = CmvsNativeSceneKind57::new(&mut rng)?;
            let frame = owner.initial_frame();
            (CmvsNativeSceneWorker::Scene57(owner), frame)
        }
        _ => return Err(invalid("unsupported native controller constructor")),
    };
    remove_slot(vm, slot);
    apply_frame(&mut vm.independent_views[0], frame);
    vm.native_scene_workers.insert(slot as u8, worker);
    vm.write_interpreter_word(2948 + 4 * slot, 1);
    vm.prng_state = rng;
    Ok(true)
}
pub(crate) fn request(
    vm: &mut CmvsPs2aVmState,
    slot: u32,
    words: [u32; 3],
) -> Result<(), CoreError> {
    if slot > 3 {
        return Ok(());
    }
    let worker = vm
        .native_scene_workers
        .get_mut(&(slot as u8))
        .ok_or_else(|| invalid("native scene request requires a live owner"))?;
    match worker {
        CmvsNativeSceneWorker::CollisionParticles(owner) if words[0] == 1 => {
            owner.parameter_words[0] = words[1]
        }
        CmvsNativeSceneWorker::Scene51(owner) => owner.request(words[0]),
        CmvsNativeSceneWorker::Scene52(owner) => owner.request(words[0]),
        CmvsNativeSceneWorker::Scene53(owner) => owner.request(words[0]),
        CmvsNativeSceneWorker::Scene54(owner) => owner.request(words[0]),
        CmvsNativeSceneWorker::Scene55(owner) => owner.request(words[0]),
        CmvsNativeSceneWorker::Scene56(owner) => owner.request(words[0]),
        CmvsNativeSceneWorker::Scene57(owner) => owner.request(words[0]),
        _ => {}
    }
    Ok(())
}
fn apply_frame(view: &mut CmvsIndependentView, frame: native_scene_frame::NativeSceneFrame) {
    if let Some(position) = frame.position_words {
        view.position_words = position;
    }
    if let Some(angles) = frame.angle_words {
        view.angle_words = angles;
    }
    if let Some(pitch) = frame.pitch_word {
        view.angle_words[0] = pitch;
    }
    if let Some(yaw) = frame.yaw_word {
        view.angle_words[1] = yaw;
    }
    if let Some(fov) = frame.field_of_view_word {
        view.field_of_view_word = fov;
    }
}
pub(crate) fn start(
    vm: &mut CmvsPs2aVmState,
    command: u16,
    words: &[u32],
    resource: Vec<u8>,
) -> Result<bool, CoreError> {
    let expected = match command {
        673 | 674 | 676 | 677 => 8,
        675 | 678 | 679 | 681 => 6,
        _ => return Err(invalid("unsupported native scene producer")),
    };
    if words.len() != expected {
        return Err(invalid("invalid native scene producer operand count"));
    }
    if words[0] > 3 {
        return Ok(false);
    }
    let mut rng = vm.prng_state;
    let worker = match command {
        673 => CmvsNativeSceneWorker::Kind1(CmvsNativeParticleKind1::new(
            resource,
            words[2..].try_into().unwrap(),
            &mut rng,
        )?),
        674 => {
            let mut parameters: [u32; 6] = words[2..].try_into().unwrap();
            parameters[5] = u32::from(parameters[5] != 0);
            CmvsNativeSceneWorker::MeshSnow(CmvsNativeSnow::new(resource, parameters, &mut rng)?)
        }
        675 => CmvsNativeSceneWorker::Kind3(CmvsNativeParticleKind3::new(
            resource,
            words[2..].try_into().unwrap(),
            &mut rng,
        )?),
        676 => {
            let mut owner =
                CmvsNativeParticleKind4::new(resource, words[2..].try_into().unwrap(), &mut rng)?;
            owner.clock_ms = vm.session_clock_millis;
            CmvsNativeSceneWorker::Kind4(owner)
        }
        677 => {
            let mut owner =
                CmvsNativeParticleKind5::new(resource, words[2..].try_into().unwrap(), &mut rng)?;
            owner.clock_ms = vm.session_clock_millis;
            CmvsNativeSceneWorker::Kind5(owner)
        }
        678 => CmvsNativeSceneWorker::CollisionParticles(CmvsNativeCollisionParticles::new(
            resource,
            words[2..].try_into().unwrap(),
            &mut rng,
        )?),
        679 => {
            let mut owner =
                CmvsNativeParticleKind7::new(resource, words[2..].try_into().unwrap(), &mut rng)?;
            owner.clock_ms = vm.session_clock_millis;
            CmvsNativeSceneWorker::Kind7(owner)
        }
        681 => {
            let mut owner = CmvsNativeParticleKind9::new(
                resource,
                words[2..].try_into().unwrap(),
                vm.native_stage_size,
                &mut rng,
            )?;
            owner.clock_ms = vm.session_clock_millis;
            CmvsNativeSceneWorker::Kind9(owner)
        }
        _ => unreachable!(),
    };
    remove_slot(vm, words[0]);
    vm.native_scene_workers.insert(words[0] as u8, worker);
    vm.prng_state = rng;
    vm.write_interpreter_word(2948 + 4 * words[0], 1);
    Ok(true)
}
pub(crate) fn retired_audio(vm: &CmvsPs2aVmState, slot: u32) -> (u32, u16) {
    let worker = u8::try_from(slot)
        .ok()
        .and_then(|slot| vm.native_scene_workers.get(&slot));
    match worker {
        Some(CmvsNativeSceneWorker::Scene51(_)) => (0x1f << 16, 1 << 10),
        Some(CmvsNativeSceneWorker::Scene52(_)) => (3 << 16, 0),
        Some(CmvsNativeSceneWorker::Scene54(_)) => (1 << 16, 0),
        Some(CmvsNativeSceneWorker::Scene55(_)) => {
            (CmvsNativeSceneKind55::RETIRED_STATIC_SOUNDS, 0)
        }
        _ => (0, 0),
    }
}
pub(crate) fn remove_slot(vm: &mut CmvsPs2aVmState, slot: u32) {
    if slot > 3 {
        return;
    }
    vm.native_scene_workers.remove(&(slot as u8));
    let key = (2948_u32 << 8) | slot;
    vm.slot_objects.remove(&key);
    vm.slot_object_seed_words.remove(&key);
    vm.write_interpreter_word(2948 + 4 * slot, 0);
}
pub(crate) fn capture(
    vm: &CmvsPs2aVmState,
) -> Result<BTreeMap<u16, SavedNativeSceneWorker>, CoreError> {
    validate(vm)?;
    vm.native_scene_workers
        .iter()
        .map(|(&slot, worker)| {
            Ok((
                u16::from(slot),
                match worker {
                    CmvsNativeSceneWorker::Kind1(owner) => {
                        SavedNativeSceneWorker::Kind1(owner.capture()?)
                    }
                    CmvsNativeSceneWorker::MeshSnow(owner) => {
                        SavedNativeSceneWorker::MeshSnow(owner.capture()?)
                    }
                    CmvsNativeSceneWorker::Kind3(owner) => {
                        SavedNativeSceneWorker::Kind3(owner.capture()?)
                    }
                    CmvsNativeSceneWorker::Kind4(owner) => {
                        SavedNativeSceneWorker::Kind4(owner.capture()?)
                    }
                    CmvsNativeSceneWorker::Kind5(owner) => {
                        SavedNativeSceneWorker::Kind5(owner.capture()?)
                    }
                    CmvsNativeSceneWorker::CollisionParticles(owner) => {
                        SavedNativeSceneWorker::CollisionParticles(owner.capture()?)
                    }
                    CmvsNativeSceneWorker::Kind7(owner) => {
                        SavedNativeSceneWorker::Kind7(owner.capture()?)
                    }
                    CmvsNativeSceneWorker::Kind9(owner) => {
                        SavedNativeSceneWorker::Kind9(owner.capture()?)
                    }
                    CmvsNativeSceneWorker::Empty => SavedNativeSceneWorker::Marker(0),
                    CmvsNativeSceneWorker::ImageShards(_) => SavedNativeSceneWorker::Marker(8),
                    CmvsNativeSceneWorker::Scene50(_) => SavedNativeSceneWorker::Marker(0x50),
                    CmvsNativeSceneWorker::Scene51(_) => SavedNativeSceneWorker::Marker(0x51),
                    CmvsNativeSceneWorker::Scene52(_) => SavedNativeSceneWorker::Marker(0x52),
                    CmvsNativeSceneWorker::Scene53(_) => SavedNativeSceneWorker::Marker(0x53),
                    CmvsNativeSceneWorker::Scene54(_) => SavedNativeSceneWorker::Marker(0x54),
                    CmvsNativeSceneWorker::Scene55(_) => SavedNativeSceneWorker::Marker(0x55),
                    CmvsNativeSceneWorker::Scene56(_) => SavedNativeSceneWorker::Marker(0x56),
                    CmvsNativeSceneWorker::Scene57(_) => SavedNativeSceneWorker::Marker(0x57),
                },
            ))
        })
        .collect()
}
pub(crate) fn restore(
    vm: &mut CmvsPs2aVmState,
    records: &BTreeMap<u16, SavedNativeSceneWorker>,
) -> Result<(), CoreError> {
    let mut rng = vm.prng_state;
    let mut workers = BTreeMap::new();
    for (&slot, record) in records {
        if slot > 3 {
            return Err(invalid("native scene worker slot exceeds three"));
        }
        let worker = match record {
            SavedNativeSceneWorker::Kind1(saved) => {
                CmvsNativeSceneWorker::Kind1(saved.rebuild(&mut rng)?)
            }
            SavedNativeSceneWorker::MeshSnow(saved) => {
                CmvsNativeSceneWorker::MeshSnow(saved.rebuild(&mut rng)?)
            }
            SavedNativeSceneWorker::Kind3(saved) => {
                CmvsNativeSceneWorker::Kind3(saved.rebuild(&mut rng)?)
            }
            SavedNativeSceneWorker::Kind4(saved) => {
                let mut owner = saved.rebuild(&mut rng)?;
                owner.clock_ms = vm.session_clock_millis;
                CmvsNativeSceneWorker::Kind4(owner)
            }
            SavedNativeSceneWorker::Kind5(saved) => {
                let mut owner = saved.rebuild(&mut rng)?;
                owner.clock_ms = vm.session_clock_millis;
                CmvsNativeSceneWorker::Kind5(owner)
            }
            SavedNativeSceneWorker::CollisionParticles(saved) => {
                CmvsNativeSceneWorker::CollisionParticles(saved.rebuild(&mut rng)?)
            }
            SavedNativeSceneWorker::Kind7(saved) => {
                let mut owner = saved.rebuild(&mut rng)?;
                owner.clock_ms = vm.session_clock_millis;
                CmvsNativeSceneWorker::Kind7(owner)
            }
            SavedNativeSceneWorker::Kind9(saved) => {
                let mut owner = saved.rebuild(vm.native_stage_size, &mut rng)?;
                owner.clock_ms = vm.session_clock_millis;
                CmvsNativeSceneWorker::Kind9(owner)
            }
            SavedNativeSceneWorker::Marker(kind) if matches!(*kind, 0 | 8 | 0x50..=0x57) => {
                CmvsNativeSceneWorker::Empty
            }
            SavedNativeSceneWorker::Marker(_) => {
                return Err(invalid("unsupported native scene marker kind"));
            }
        };
        workers.insert(slot as u8, worker);
    }
    vm.native_scene_workers = workers;
    vm.prng_state = rng;
    vm.slot_objects.retain(|key, _| *key >> 8 != 2948);
    vm.slot_object_seed_words.retain(|key, _| *key >> 8 != 2948);
    for slot in 0..4 {
        vm.write_interpreter_word(
            2948 + 4 * slot,
            u32::from(records.contains_key(&(slot as u16))),
        );
    }
    Ok(())
}
fn advance_workers(
    vm: &mut CmvsPs2aVmState,
    delta_ms: u32,
    vertices: &[[f32; 4]],
) -> Result<Vec<native_scene_frame::NativeSceneSound>, CoreError> {
    let mut audio = Vec::new();
    let volume = vm
        .interpreter_words
        .get(&1560)
        .copied()
        .unwrap_or(256)
        .wrapping_mul(vm.interpreter_words.get(&1568).copied().unwrap_or(256))
        >> 8;
    let now = vm.session_clock_millis;
    for worker in vm.native_scene_workers.values_mut() {
        match worker {
            CmvsNativeSceneWorker::Kind1(owner) => owner.advance(delta_ms, &mut vm.prng_state)?,
            CmvsNativeSceneWorker::MeshSnow(owner) => owner.advance(
                delta_ms,
                vm.independent_views[0].position_words.map(f32::from_bits),
            )?,
            CmvsNativeSceneWorker::Kind3(owner) => owner.advance(delta_ms)?,
            CmvsNativeSceneWorker::Kind4(owner) => {
                owner.advance(now.wrapping_sub(owner.clock_ms), &mut vm.prng_state)?
            }
            CmvsNativeSceneWorker::Kind5(owner) => owner.advance(
                now.wrapping_sub(owner.clock_ms),
                delta_ms,
                &mut vm.prng_state,
            )?,
            CmvsNativeSceneWorker::Kind7(owner) => {
                owner.advance(now.wrapping_sub(owner.clock_ms), &mut vm.prng_state)?
            }
            CmvsNativeSceneWorker::Kind9(owner) => {
                owner.advance(now.wrapping_sub(owner.clock_ms), &mut vm.prng_state)?
            }
            CmvsNativeSceneWorker::CollisionParticles(owner) => {
                owner.advance(delta_ms, vertices, &mut vm.prng_state)?
            }
            CmvsNativeSceneWorker::ImageShards(owner) => owner.advance(now)?,
            CmvsNativeSceneWorker::Scene50(owner) => owner.advance([
                vm.independent_views[0].position_words[0],
                vm.independent_views[0].position_words[2],
            ])?,
            CmvsNativeSceneWorker::Scene51(owner) => {
                let frame = owner.advance(delta_ms, volume)?;
                apply_frame(&mut vm.independent_views[0], frame);
                audio.extend(frame.audio.into_iter().flatten());
            }
            CmvsNativeSceneWorker::Scene52(owner) => {
                let frame = owner.advance(delta_ms, volume)?;
                apply_frame(&mut vm.independent_views[0], frame);
                audio.extend(frame.audio.into_iter().flatten());
            }
            CmvsNativeSceneWorker::Scene53(owner) => {
                let frame = owner.advance(delta_ms, volume)?;
                apply_frame(&mut vm.independent_views[0], frame);
                audio.extend(frame.audio.into_iter().flatten());
            }
            CmvsNativeSceneWorker::Scene54(owner) => {
                let frame = owner.advance(delta_ms, volume)?;
                apply_frame(&mut vm.independent_views[0], frame);
                audio.extend(frame.audio.into_iter().flatten());
            }
            CmvsNativeSceneWorker::Scene55(owner) => {
                let frame = owner.advance(delta_ms, volume)?;
                audio.extend(frame.audio.into_iter().flatten());
            }
            CmvsNativeSceneWorker::Scene56(owner) => {
                let frame = owner.advance(delta_ms, volume)?;
                audio.extend(frame.audio.into_iter().flatten());
            }
            CmvsNativeSceneWorker::Scene57(owner) => {
                let frame = owner.advance(delta_ms, volume)?;
                apply_frame(&mut vm.independent_views[0], frame);
                audio.extend(frame.audio.into_iter().flatten());
            }
            CmvsNativeSceneWorker::Empty => {}
        }
    }
    Ok(audio)
}
pub(crate) fn prepare_draw(vm: &mut CmvsPs2aVmState, delta_ms: u32) -> Result<(), CoreError> {
    for worker in vm.native_scene_workers.values_mut() {
        worker.prepare_draw(delta_ms)?;
    }
    Ok(())
}
pub(crate) fn tick(
    vm: &mut CmvsPs2aVmState,
    delta_ms: u32,
) -> Result<Vec<native_scene_frame::NativeSceneSound>, CoreError> {
    if vm.native_scene_workers.is_empty() {
        return Ok(Vec::new());
    }
    if vm
        .native_scene_workers
        .values()
        .any(|worker| matches!(worker, CmvsNativeSceneWorker::CollisionParticles(_)))
    {
        let mut vertices = [[0.0; 4]; 1024];
        let count = native_hit_polygon::collect_vertices(vm, &mut vertices)?;
        advance_workers(vm, delta_ms, &vertices[..count])
    } else {
        advance_workers(vm, delta_ms, &[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(kind: u16, parameters: &[u32]) -> SavedNativeSceneWorker {
        let mut bytes = vec![1, 0];
        bytes.extend_from_slice(&kind.to_le_bytes());
        for word in parameters {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        let mut resource = [0; 32];
        resource[..5].copy_from_slice(b"p.pb3");
        bytes.extend_from_slice(&resource);
        SavedNativeSceneWorker::decode(&bytes).unwrap()
    }

    #[test]
    fn mixed_slots_preserve_native_random_order_and_private_particle_phase() {
        let records = BTreeMap::from([
            (0, record(1, &[0, 0, 0, 0, 1.0_f32.to_bits(), 50])),
            (
                1,
                record(3, &[0, 2.0_f32.to_bits(), 1000.0_f32.to_bits(), 1]),
            ),
            (
                2,
                record(
                    2,
                    &[
                        2.0_f32.to_bits(),
                        3.0_f32.to_bits(),
                        1000.0_f32.to_bits(),
                        20.0_f32.to_bits(),
                        1,
                        0,
                    ],
                ),
            ),
            (
                3,
                record(6, &[0, 10.0_f32.to_bits(), 1000.0_f32.to_bits(), 1]),
            ),
        ]);
        let mut vm = CmvsPs2aVmState::new(0);
        restore(&mut vm, &records).unwrap();
        // Kind1 consumes zero constructor draws; kind3/kind2/kind6 consume4/4/5.
        assert_eq!(vm.prng_state, 3673273548);
        let CmvsNativeSceneWorker::Kind3(kind3) = &vm.native_scene_workers[&1] else {
            panic!("kind3")
        };
        assert_eq!(kind3.particles[0].position_words[0], 0xc09f997f);
        tick(&mut vm, 60).unwrap();
        assert_eq!(vm.prng_state, 2800339366);
        let CmvsNativeSceneWorker::Kind1(kind1) = &vm.native_scene_workers[&0] else {
            panic!("kind1")
        };
        assert_eq!(kind1.draw_count, 1);
        let index = usize::from(kind1.draw_order[0]);
        let position = kind1.particles[index].position_words;
        assert_eq!([position[0], position[2]], [0x3e0a9920, 0x40428e85]);
        let encoded = postcard::to_allocvec(&vm).unwrap();
        let mut private: CmvsPs2aVmState = postcard::from_bytes(&encoded).unwrap();
        validate_cmvs390_vm_state(&private).unwrap();
        let mut native = CmvsPs2aVmState::new(0);
        restore(&mut native, &capture(&vm).unwrap()).unwrap();
        tick(&mut private, 40).unwrap();
        tick(&mut native, 40).unwrap();
        let CmvsNativeSceneWorker::Kind1(private) = &private.native_scene_workers[&0] else {
            panic!("private kind1")
        };
        let CmvsNativeSceneWorker::Kind1(native) = &native.native_scene_workers[&0] else {
            panic!("native kind1")
        };
        assert_eq!(private.draw_count, 1);
        assert_eq!(
            private.particles[index].position_words[1],
            (f32::from_bits(position[1]) - 0.04_f32).to_bits()
        );
        assert_eq!(native.draw_count, 0);
    }

    #[test]
    fn absolute_birth_clock_and_independent_atlas_clock_survive_restore() {
        let mut vm = CmvsPs2aVmState::new(0);
        vm.session_clock_millis = 1000;
        start(
            &mut vm,
            676,
            &[0, 0, 0x790, 0, 0, 0, 1.0_f32.to_bits(), 50],
            b"rain.pb3".to_vec(),
        )
        .unwrap();
        tick(&mut vm, 16).unwrap();
        let CmvsNativeSceneWorker::Kind4(owner) = &vm.native_scene_workers[&0] else {
            panic!("kind4")
        };
        assert_eq!(owner.draw_count, 0);
        vm.session_clock_millis = 1060;
        tick(&mut vm, 60).unwrap();
        let CmvsNativeSceneWorker::Kind4(owner) = &vm.native_scene_workers[&0] else {
            panic!("kind4")
        };
        assert_eq!(owner.draw_count, 1);
        // Undefined flag classes copy the birth-clock bits, not its f32 value.
        let index = usize::from(owner.draw_indices()[0]);
        assert_eq!(owner.particles[index].position_words[0], 1050);
        start(
            &mut vm,
            677,
            &[
                1,
                0,
                0x40000000,
                0,
                5.0_f32.to_bits(),
                0,
                10000.0_f32.to_bits(),
                1,
            ],
            b"atlas.pb3".to_vec(),
        )
        .unwrap();
        let CmvsNativeSceneWorker::Kind5(owner) = &vm.native_scene_workers[&1] else {
            panic!("kind5")
        };
        let position = owner.particles[0].position_words;
        tick(&mut vm, 16).unwrap();
        tick(&mut vm, 101).unwrap();
        let CmvsNativeSceneWorker::Kind5(owner) = &vm.native_scene_workers[&1] else {
            panic!("kind5")
        };
        assert_eq!(owner.particles[0].position_words, position);
        assert_eq!(owner.draw_basic(0).unwrap().rect_words.unwrap()[1], 20);
        let bytes = postcard::to_allocvec(&vm).unwrap();
        let mut private: CmvsPs2aVmState = postcard::from_bytes(&bytes).unwrap();
        private.session_clock_millis = 1100;
        tick(&mut private, 40).unwrap();
        let CmvsNativeSceneWorker::Kind4(owner) = &private.native_scene_workers[&0] else {
            panic!("kind4")
        };
        assert_eq!(owner.clock_ms, 1100);
        assert_eq!(owner.draw_count, 1);
        let mut native = CmvsPs2aVmState::new(0);
        native.session_clock_millis = 5000;
        restore(&mut native, &capture(&vm).unwrap()).unwrap();
        tick(&mut native, 16).unwrap();
        let CmvsNativeSceneWorker::Kind4(owner) = &native.native_scene_workers[&0] else {
            panic!("kind4")
        };
        assert_eq!(owner.clock_ms, 5000);
        assert_eq!(owner.draw_count, 0);
        let CmvsNativeSceneWorker::Kind5(owner) = &native.native_scene_workers[&1] else {
            panic!("kind5")
        };
        assert_eq!(owner.clock_ms, 5000);
        assert_eq!(owner.particles[0].animation_frame, 0);
    }

    #[test]
    fn flash_and_fixedpoint_clocks_restore_with_current_native_extent() {
        let mut vm = CmvsPs2aVmState::new(0);
        vm.native_stage_size = [64, 48];
        vm.session_clock_millis = 1000;
        start(&mut vm, 681, &[1, 0, 0, 160, 120, 1], b"flat.pb3".to_vec()).unwrap();
        start(
            &mut vm,
            679,
            &[0, 0, 0x0001030a, 80.0_f32.to_bits(), 1e9_f32.to_bits(), 1],
            b"flash.pb3".to_vec(),
        )
        .unwrap();
        tick(&mut vm, 16).unwrap();
        let CmvsNativeSceneWorker::Kind9(flat) = &vm.native_scene_workers[&1] else {
            panic!("flat")
        };
        assert_eq!(flat.particles[0].fixed_position_words, [41 << 10, 35 << 10]);
        vm.session_clock_millis = 1500;
        tick(&mut vm, 500).unwrap();
        let CmvsNativeSceneWorker::Kind7(flash) = &vm.native_scene_workers[&0] else {
            panic!("flash")
        };
        assert_eq!(flash.alpha, 51);
        let bytes = postcard::to_allocvec(&vm).unwrap();
        let mut private: CmvsPs2aVmState = postcard::from_bytes(&bytes).unwrap();
        private.native_stage_size = [32, 24];
        private.session_clock_millis = 1900;
        tick(&mut private, 400).unwrap();
        let CmvsNativeSceneWorker::Kind7(flash) = &private.native_scene_workers[&0] else {
            panic!("flash")
        };
        assert_eq!(flash.alpha, 153);
        let CmvsNativeSceneWorker::Kind9(flat) = &private.native_scene_workers[&1] else {
            panic!("flat")
        };
        assert_eq!(flat.extent, [64, 48]);
        assert_eq!(flat.particles[0].fixed_position_words[1], (35 << 10) + 9000);
        let mut native = CmvsPs2aVmState::new(0);
        native.native_stage_size = [32, 24];
        native.session_clock_millis = 9000;
        restore(&mut native, &capture(&vm).unwrap()).unwrap();
        tick(&mut native, 16).unwrap();
        let CmvsNativeSceneWorker::Kind9(flat) = &native.native_scene_workers[&1] else {
            panic!("flat")
        };
        assert_eq!(flat.clock_ms, 9000);
        assert_eq!(flat.extent, [32, 24]);
        native.session_clock_millis = 9400;
        tick(&mut native, 400).unwrap();
        let CmvsNativeSceneWorker::Kind7(flash) = &native.native_scene_workers[&0] else {
            panic!("flash")
        };
        assert_eq!(flash.alpha, 25);
    }
}

#[cfg(test)]
#[path = "native_scene_worker/controller_tests.rs"]
mod controller_tests;
