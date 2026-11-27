use super::*;
use crate::core_catalog::CoreCatalogEntry;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredMatch {
    pub rules_key: String,
    pub plugin_ids: Vec<String>,
}

/// Effective declarations from validated core packages.
/// The Manager validates the package schema and supplies the canonical key.
#[derive(Default)]
pub struct DeclaredRules {
    key: String,
    root: Node,
}

#[derive(Default)]
struct Node {
    children: BTreeMap<String, Node>,
    rules: Vec<Rule>,
}

struct Rule {
    core: &'static CoreCatalogEntry,
    offset: u64,
    magic: Vec<u8>,
}

impl DeclaredRules {
    pub fn new(key: String) -> Self {
        Self {
            key,
            ..Self::default()
        }
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    /// `path` and `magic` must come from the validated core recognition schema.
    pub fn add(
        &mut self,
        core: &'static CoreCatalogEntry,
        path: &str,
        offset: u64,
        magic: Vec<u8>,
    ) {
        let mut node = &mut self.root;
        for part in path.split('/') {
            if !node.children.contains_key(part) {
                node.children.insert(part.into(), Node::default());
            }
            node = node
                .children
                .get_mut(part)
                .expect("declared path component");
        }
        node.rules.push(Rule {
            core,
            offset,
            magic,
        });
    }

    pub fn detect_game_directory(
        &self,
        root: &Path,
        cancellation: &NativeCancellation,
    ) -> DetectionResult<Vec<EngineDetection>> {
        self.inspect_game_directory(root, cancellation)
            .map(|(matches, _)| matches)
    }

    /// Returns matches and whether traversal inspected an ordinary file.
    pub fn inspect_game_directory(
        &self,
        root: &Path,
        cancellation: &NativeCancellation,
    ) -> DetectionResult<(Vec<EngineDetection>, bool)> {
        detect_tree_at(&LocalTree, root, cancellation, Some(self))
    }

    /// The same read-only inspection against a provider tree.
    pub fn inspect_game_tree<T: DetectionTree>(
        &self,
        tree: &T,
        cancellation: &NativeCancellation,
    ) -> DetectionResult<(Vec<EngineDetection>, bool)> {
        detect_tree_at(tree, Path::new(""), cancellation, Some(self))
    }

    pub(super) fn inspect<T: DetectionTree>(
        &self,
        scan: &mut Scan<'_, T>,
        root: &Path,
    ) -> DetectionResult<()> {
        if self.root.children.is_empty() {
            return Ok(());
        }
        self.directory(scan, &mut root.to_owned(), &self.root)?;
        for found in &mut scan.matches {
            if let Some(declared) = &mut found.declared {
                declared.plugin_ids.sort_unstable();
            }
        }
        Ok(())
    }

    fn directory<T: DetectionTree>(
        &self,
        scan: &mut Scan<'_, T>,
        path: &mut PathBuf,
        node: &Node,
    ) -> DetectionResult<()> {
        let mut seen = vec![(None, false); node.children.len()];
        scan.entries(path, |name, kind| {
            if let Some(index) = node
                .children
                .keys()
                .position(|expected| name.eq_ignore_ascii_case(expected))
            {
                let (entry, ambiguous) = &mut seen[index];
                *ambiguous |= entry.replace((name, kind)).is_some();
            }
            Ok(())
        })?;
        for ((entry, ambiguous), child) in seen.into_iter().zip(node.children.values()) {
            if ambiguous {
                continue;
            }
            let Some((name, kind)) = entry else {
                continue;
            };
            path.push(name);
            if kind == EntryKind::Directory && !child.children.is_empty() {
                self.directory(scan, path, child)?;
            } else if kind == EntryKind::File && !child.rules.is_empty() {
                self.file(scan, path, &child.rules)?;
            }
            path.pop();
        }
        Ok(())
    }

    fn file<T: DetectionTree>(
        &self,
        scan: &mut Scan<'_, T>,
        path: &Path,
        rules: &[Rule],
    ) -> DetectionResult<()> {
        scan.checkpoint()?;
        scan.count_file()?;
        let mut source = None;
        // Reuse one open and one buffer for all clauses for the same file.
        let mut buffer = Vec::new();
        for rule in rules {
            scan.checkpoint()?;
            let core = rule.core;
            let prior = scan
                .matches
                .iter_mut()
                .find(|found| found.family_id == core.family_id);
            if prior.as_ref().is_some_and(|found| {
                found.declared.as_ref().is_none_or(|declared| {
                    declared.plugin_ids.iter().any(|id| id == core.plugin_id)
                })
            }) {
                // A built-in match already offers the whole family.
                continue;
            }
            if !rule.magic.is_empty() {
                if source.is_none() {
                    source = scan.open(path, 0)?;
                }
                let Some(file) = source.as_mut() else {
                    continue;
                };
                let length = rule.magic.len();
                if rule
                    .offset
                    .checked_add(length as u64)
                    .is_none_or(|end| end > file.length)
                {
                    continue;
                }
                buffer.resize(length, 0);
                scan.read_at(file, rule.offset, &mut buffer)?;
                if buffer != rule.magic {
                    continue;
                }
            }
            if let Some(found) = scan
                .matches
                .iter_mut()
                .find(|found| found.family_id == core.family_id)
            {
                found
                    .declared
                    .as_mut()
                    .expect("declared family match")
                    .plugin_ids
                    .push(core.plugin_id.into());
            } else {
                scan.matches.push(EngineDetection {
                    family_id: core.family_id.into(),
                    format: String::new(),
                    declared: Some(DeclaredMatch {
                        rules_key: self.key.clone(),
                        plugin_ids: vec![core.plugin_id.into()],
                    }),
                });
            }
        }
        Ok(())
    }
}
