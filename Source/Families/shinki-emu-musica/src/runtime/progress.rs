use super::*;
pub(crate) fn validate(unlocks: &[Hash256]) -> Result<(), MusicaRuntimeError> {
    if unlocks.len() > 4096 || unlocks.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(MusicaRuntimeError::State);
    }
    Ok(())
}
pub(super) fn record(state: &mut MusicaRuntimeState, key: &str, value: i64) {
    if value != 1 {
        return;
    }
    let id = Hash256::from_sha256(key.as_bytes());
    if let Err(index) = state.gallery_unlocks.binary_search(&id) {
        state.gallery_unlocks.insert(index, id);
    }
}
/// A hash cannot recover its source variable name. Resolve only against actual
/// script/configuration declarations and return every unresolved record intact.
pub(crate) fn unlocked_flags<'a>(
    unlocks: &[Hash256],
    candidates: &'a [String],
) -> Result<(Vec<&'a str>, Vec<Hash256>), MusicaRuntimeError> {
    validate(unlocks)?;
    let mut names = BTreeMap::new();
    for name in candidates {
        names
            .entry(Hash256::from_sha256(name.as_bytes()))
            .or_insert(name.as_str());
    }
    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();
    for hash in unlocks {
        match names.get(hash) {
            Some(name) => resolved.push(*name),
            None => unresolved.push(*hash),
        }
    }
    Ok((resolved, unresolved))
}
impl MusicaVm {
    pub(crate) fn progress_flag_names(&self) -> Vec<String> {
        // Priority 1: declarations in the original entry script.
        let mut names = self
            .script
            .lines
            .iter()
            .filter_map(|line| match &line.kind {
                ScLineKind::Command { command } if command.opcode == "setglobal" => {
                    match command.operands.first() {
                        Some(ScOperand::Symbol { value }) => Some(value.clone()),
                        _ => None,
                    }
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        // Priority 2: frontend requirements recovered/configured for this release.
        for title in &self.frontend.titles {
            names.extend(title.requires.keys().cloned());
        }
        for entry in self
            .frontend
            .replays
            .iter()
            .chain(&self.frontend.movies)
            .chain(&self.frontend.cg)
            .chain(&self.frontend.bgm)
        {
            names.extend(entry.requires.keys().cloned());
        }
        names
    }
    pub(crate) fn replace_global_variables(&mut self, globals: BTreeMap<String, i64>) {
        for (key, value) in &globals {
            record(&mut self.state, key, *value);
        }
        self.state.global_variables = globals;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn progress_uses_assignment_values_independently_of_variable_names() {
        for names in [["locked", "bonus", "route"], ["x", "y", "z"]] {
            let source = format!(
                ".setglobal {} = 0\r\n.setglobal {} = 1\r\n.setglobal {} = 1\r\n.setglobal {} = 1\r\n.end\r\n",
                names[0], names[1], names[2], names[2]
            );
            let mut vm = MusicaVm::new(
                "musica:/scr/test.sc".into(),
                Hash256::from_sha256(source.as_bytes()),
                crate::parse_sc(
                    source.as_bytes(),
                    &crate::ScOpcodeCatalog::observed_musica(),
                )
                .unwrap(),
                0,
            )
            .unwrap();
            vm.step(1).unwrap();
            assert_eq!(vm.state().gallery_unlocks.len(), 2);
            assert_eq!(vm.state().global_variables[names[0]], 0);
            for name in &names[1..] {
                assert!(
                    vm.state()
                        .gallery_unlocks
                        .contains(&Hash256::from_sha256(name.as_bytes()))
                );
            }
            let saved = MusicaVm::checked_test_state(vm.state()).unwrap();
            let mut corrupt = vm.state().clone();
            corrupt.gallery_unlocks.push(corrupt.gallery_unlocks[0]);
            assert!(vm.install_test_state(&corrupt, 1).is_err());
            assert_eq!(vm.state(), &saved);
            let declared = vm.progress_flag_names();
            let (mapped, pending) = unlocked_flags(&vm.state().gallery_unlocks, &declared).unwrap();
            assert_eq!(mapped.len(), 2);
            assert!(pending.is_empty());
            vm.replace_global_variables(BTreeMap::from([("another_flag".into(), 1)]));
            assert_eq!(vm.state().gallery_unlocks.len(), 3);
            assert!(!vm.state().global_variables.contains_key(names[1]));
        }
    }
}
