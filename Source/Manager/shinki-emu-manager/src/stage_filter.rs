use std::path::Path;

use shinki_emu_manager::effects::{FilterEngine, FilterError, FilterTargets, PreparedFilter};

pub(super) enum PendingFilter {
    Prepared(PreparedFilter),
    Ready(ActiveFilter),
}

pub(super) struct ActiveFilter {
    pub(super) engine: FilterEngine,
    pub(super) targets: FilterTargets,
}

impl ActiveFilter {
    pub(super) fn build(
        device: &wgpu::Device,
        dxc_path: &Path,
        prepared: PreparedFilter,
        width: u32,
        height: u32,
    ) -> Result<Self, FilterError> {
        let (engine, targets) = FilterEngine::build(device, dxc_path, prepared, width, height)?;
        Ok(Self { engine, targets })
    }

    pub(super) fn resize(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> Result<(), FilterError> {
        if !self.targets.matches_input(width, height) {
            self.targets = self.engine.resize_targets(device, width, height)?;
        }
        Ok(())
    }
}
