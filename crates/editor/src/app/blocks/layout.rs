//! Revision-owned Block data and stable geometry; animation still uses its frozen layout.
use super::*;

pub(in crate::app) struct Snapshot {
    revision: u64,
    pub source: Rc<str>,
    pub projection: Rc<EiyashouProjection>,
    pub lines: keine_loader::SourceLineIndex,
    pub order: Arc<Vec<usize>>,
    pub endings: HashSet<usize>,
    pub lookup: HashMap<usize, (usize, usize)>,
    pub text_lookup: HashMap<usize, usize>,
    pub number_width: f32,
}

impl Snapshot {
    pub fn block_at_position(
        &self,
        line: usize,
        column: usize,
    ) -> Option<&crate::projection::BlockCard> {
        let offset = self.lines.offset(&self.source, line, column);
        block_at_offset(&self.projection, offset).map(|(_, block)| block)
    }
}

pub(in crate::app) struct SceneRows {
    pub indices: Vec<usize>,
    pub prefix: Vec<f32>,
    pub body_height: f32,
}

pub(in crate::app) struct Geometry {
    pub scenes: Vec<SceneRows>,
    pub positions: HashMap<usize, f32>,
    pub marks: Rc<Vec<minimap::Mark>>,
    pub rows: Vec<(usize, f32, f32)>,
}

#[derive(Default)]
pub(in crate::app) struct Cache {
    snapshot: Option<Rc<Snapshot>>,
    geometry: Option<Rc<Geometry>>,
    collapsed: HashSet<String>,
    heights: HashMap<usize, f32>,
    pub positioned: bool,
    editor_text: Option<(
        gpui_kit::base::input::Rope,
        Rc<Vec<crate::authoring::DialogueEntry>>,
    )>,
    selected: HashSet<usize>,
    cursor: Option<(usize, usize)>,
    marks: Option<Rc<Vec<minimap::Mark>>>,
}

impl Cache {
    pub fn snapshot(&mut self, document: &DocumentHandle, frozen: Option<&str>) -> Rc<Snapshot> {
        let document = document.borrow();
        let current = self.snapshot.as_ref().is_some_and(|snapshot| {
            frozen.map_or(snapshot.revision == document.revision(), |source| {
                snapshot.source.as_ref() == source
            })
        });
        if !current {
            let source: Rc<str> = frozen.unwrap_or(document.contents()).into();
            let projection = if source.as_ref() == document.contents() {
                document.projection()
            } else {
                Rc::new(EiyashouProjection::parse(&source))
            };
            let mut lookup = HashMap::new();
            let mut order = Vec::new();
            let mut text_lookup = HashMap::new();
            let mut endings = HashSet::new();
            let mut maximum = 9999;
            for (scene_index, scene) in projection.scenes.iter().enumerate() {
                let mut count = 0;
                for (block_index, block) in scene.blocks.iter().enumerate() {
                    lookup.insert(block.source_range.start, (scene_index, block_index));
                    if let Some(range) = &block.text_range {
                        text_lookup.insert(range.start, block.source_range.start);
                    }
                    if block.is_textbox_ending() {
                        endings.extend(block.lifetime_owner);
                    } else {
                        order.push(block.source_range.start);
                        count += 1;
                    }
                }
                maximum = maximum.max(count);
            }
            self.snapshot = Some(Rc::new(Snapshot {
                revision: document.revision(),
                lines: keine_loader::SourceLineIndex::new(&source),
                source,
                projection,
                order: Arc::new(order),
                endings,
                lookup,
                text_lookup,
                number_width: maximum.to_string().len() as f32 * 7.,
            }));
            self.geometry = None;
            self.heights.clear();
            self.marks = None;
            self.positioned = false;
        }
        self.snapshot.as_ref().unwrap().clone()
    }

    pub fn dialogues(
        &mut self,
        text: &gpui_kit::base::input::Rope,
        document: &DocumentHandle,
        relative: &Path,
    ) -> Rc<Vec<crate::authoring::DialogueEntry>> {
        if let Some((previous, dialogues)) = &self.editor_text
            && previous == text
        {
            return dialogues.clone();
        }
        let source = text.to_string();
        let dialogues = if document.borrow().contents() == source {
            document.borrow().dialogues()
        } else {
            Rc::new(dialogues_for_source(relative, &source))
        };
        self.editor_text = Some((text.clone(), dialogues.clone()));
        dialogues
    }

    pub fn geometry(
        &mut self,
        snapshot: &Snapshot,
        collapsed: &HashSet<String>,
        heights: &HashMap<usize, f32>,
    ) -> Rc<Geometry> {
        let changed = self.geometry.is_none()
            || self.collapsed != *collapsed
            || self.heights.len() != heights.len()
            || heights
                .iter()
                .any(|(id, height)| self.heights.get(id) != Some(height));
        if changed {
            self.collapsed.clone_from(collapsed);
            self.heights.clone_from(heights);
            let mut scenes = Vec::new();
            let mut positions = HashMap::new();
            let mut marks = Vec::new();
            let mut rows = Vec::new();
            let mut offset = 40.;
            for scene in &snapshot.projection.scenes {
                let closed = collapsed.contains(&scene.name);
                marks.push(minimap::Mark {
                    top: offset,
                    height: 32.,
                    depth: 0,
                    width: 60.,
                    color: PRIMARY,
                    selected: false,
                    error: closed && scene.blocks.iter().any(|block| block.read_only),
                });
                offset += 36.;
                let mut indices = Vec::new();
                let mut prefix = vec![0.];
                for (index, block) in scene
                    .blocks
                    .iter()
                    .enumerate()
                    .filter(|(_, block)| !block.is_textbox_ending())
                {
                    let height = heights
                        .get(&block.source_range.start)
                        .copied()
                        .unwrap_or_else(|| default_height(block));
                    let top = offset + if closed { 0. } else { *prefix.last().unwrap() };
                    positions.insert(block.source_range.start, top);
                    if !closed {
                        marks.push(minimap::Mark::block(block, top, height, false));
                        rows.push((block.source_range.start, top, height));
                    }
                    indices.push(index);
                    prefix.push(prefix.last().unwrap() + height + 4.);
                }
                let body_height = (prefix.last().unwrap() - 4.).max(0.);
                if !closed {
                    offset += body_height;
                }
                offset += 4.;
                scenes.push(SceneRows {
                    indices,
                    prefix,
                    body_height,
                });
            }
            self.geometry = Some(Rc::new(Geometry {
                scenes,
                positions,
                marks: Rc::new(marks),
                rows,
            }));
            self.marks = None;
            self.positioned = false;
        }
        self.geometry.as_ref().unwrap().clone()
    }

    pub fn marks(
        &mut self,
        snapshot: &Snapshot,
        geometry: &Geometry,
        selected: &HashSet<usize>,
        cursor: Option<(usize, usize)>,
    ) -> Rc<Vec<minimap::Mark>> {
        if self.marks.is_none() || self.selected != *selected || self.cursor != cursor {
            self.selected.clone_from(selected);
            self.cursor = cursor;
            let mut marks = geometry.marks.as_ref().clone();
            let mut index = 0;
            for scene in &snapshot.projection.scenes {
                let closed = self.collapsed.contains(&scene.name);
                let line = snapshot
                    .lines
                    .span(&snapshot.source, scene.name_range.start)
                    .line
                    - 1;
                marks[index].selected = cursor
                    .is_some_and(|(selected_line, _)| selected_line == line)
                    || closed
                        && scene.blocks.iter().any(|block| {
                            selected.contains(&block.source_range.start)
                                || cursor
                                    .is_some_and(|(_, start)| block.source_range.start == start)
                        });
                index += 1;
                if !closed {
                    for block in scene
                        .blocks
                        .iter()
                        .filter(|block| !block.is_textbox_ending())
                    {
                        marks[index].selected = selected.contains(&block.source_range.start)
                            || cursor.is_some_and(|(_, start)| block.source_range.start == start);
                        index += 1;
                    }
                }
            }
            self.marks = Some(Rc::new(marks));
        }
        self.marks.as_ref().unwrap().clone()
    }
}

fn default_height(block: &crate::projection::BlockCard) -> f32 {
    match block.kind {
        BlockKind::Narration | BlockKind::Dialogue { .. } => {
            let speaker = if matches!(block.kind, BlockKind::Dialogue { .. }) {
                20.
            } else {
                0.
            };
            speaker + 38. + block.text_rows.saturating_sub(1) as f32 * 20.
        }
        BlockKind::Choice
        | BlockKind::ChoiceOption
        | BlockKind::Conditional
        | BlockKind::ElseIf
        | BlockKind::Else
        | BlockKind::Loop => 44.,
        _ => 38.,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(source: &str) -> (DocumentHandle, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "keine-layout-{}-{}",
            std::process::id(),
            source.len()
        ));
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(root.join("scripts/main.shou"), source).unwrap();
        fs::write(
            root.join("config.yaml"),
            "title: Fixture\nadapter:\n  script: keine\nscript:\n  version: 2\n  entry: first\n",
        )
        .unwrap();
        let mut manager =
            crate::document::DocumentManager::new(root.clone(), root.join("recovery")).unwrap();
        (manager.open("scripts/main.shou").unwrap(), root)
    }

    #[test]
    fn cache_tracks_source_heights_collapse_selection_and_frozen_drag() {
        let source = "scene first {\n  hero: \"你好\",\n  choice {\n    \"继续\": { goto(second) },\n    \"退出\": { goto(second) }\n  }\n}\nscene second {\n  wait(10ms)\n}";
        assert!(
            keine_loader::parse_native_document(source)
                .diagnostics
                .is_empty()
        );
        let (document, root) = document(source);
        let mut cache = Cache::default();
        let snapshot = cache.snapshot(&document, None);
        assert!(Rc::ptr_eq(&snapshot, &cache.snapshot(&document, None)));
        let mut heights = HashMap::new();
        let mut collapsed = HashSet::new();
        let geometry = cache.geometry(&snapshot, &collapsed, &heights);
        assert!(Rc::ptr_eq(
            &geometry,
            &cache.geometry(&snapshot, &collapsed, &heights)
        ));
        assert!(
            geometry
                .rows
                .windows(2)
                .all(|pair| pair[0].1 + pair[0].2 <= pair[1].1)
        );
        for scene in &geometry.scenes {
            assert_eq!(scene.prefix.len(), scene.indices.len() + 1);
            assert_eq!(
                scene.body_height,
                (scene.prefix.last().unwrap() - 4.).max(0.)
            );
        }
        let id = snapshot.order[0];
        heights.insert(id, 100.);
        let taller = cache.geometry(&snapshot, &collapsed, &heights);
        assert!(!Rc::ptr_eq(&geometry, &taller));
        assert_eq!(taller.rows[0].2, 100.);
        assert_eq!(
            taller.rows[1].1 - geometry.rows[1].1,
            100. - geometry.rows[0].2
        );
        heights.clear();
        let restored = cache.geometry(&snapshot, &collapsed, &heights);
        assert_eq!(restored.rows, geometry.rows);
        heights.insert(id, 100.);
        let taller = cache.geometry(&snapshot, &collapsed, &heights);
        let selected = HashSet::from([id]);
        let marks = cache.marks(&snapshot, &taller, &selected, None);
        assert!(marks[1].selected);
        assert!(Rc::ptr_eq(
            &marks,
            &cache.marks(&snapshot, &taller, &selected, None)
        ));
        collapsed.insert("first".into());
        let closed = cache.geometry(&snapshot, &collapsed, &heights);
        assert_eq!(closed.rows.len(), 1);
        assert_eq!(closed.rows[0].1, 116.);
        document
            .borrow_mut()
            .replace_contents("scene changed { wait(20ms), }".into())
            .unwrap();
        assert!(Rc::ptr_eq(
            &snapshot,
            &cache.snapshot(&document, Some(source))
        ));
        let changed = cache.snapshot(&document, None);
        assert!(!Rc::ptr_eq(&snapshot, &changed));
        assert_eq!(changed.projection.scenes[0].name, "changed");
        fs::remove_dir_all(root).unwrap();
    }

    mod benchmark {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/bench/editor/blocks.rs"
        ));
    }
}
