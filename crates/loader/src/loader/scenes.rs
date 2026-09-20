use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use anyhow::{Context, Result};
use keine_core::{
    Action, ChoiceTarget, EiyashouExpr, EiyashouListOperation, EiyashouPlace, EiyashouText,
    EiyashouTextPart,
};

use crate::{
    ContentMount, ContentProject, Diagnostic, DiagnosticLevel, ResourceRef, SceneRef,
    ScriptLanguageRegistry, adapter::eiyashou_semantic_diagnostics, source_input::SourceReader,
};

#[derive(Debug, Clone, PartialEq)]
pub struct LoadedScene {
    pub name: String,
    pub path: PathBuf,
    pub actions: Vec<Action>,
    /// Source position for every action, kept parallel to `actions` for
    /// editor-driven seek and diagnostics.
    pub action_spans: Vec<crate::SourceSpan>,
    pub diagnostics: Vec<Diagnostic>,
    pub resources: Vec<ResourceRef>,
    pub sub_scenes: Vec<SceneRef>,
}

/// Loads every script layer in stable order. A scene in a later content source
/// replaces one with the same name from an earlier source.
pub fn load_scenes(project: &ContentProject) -> Result<Vec<LoadedScene>> {
    load_scenes_with(project, &ScriptLanguageRegistry::default())
}

pub fn load_scenes_with(
    project: &ContentProject,
    languages: &ScriptLanguageRegistry,
) -> Result<Vec<LoadedScene>> {
    if let Some(loader) = project.scene_loader() {
        let mut scenes = loader.load(&project.root)?;
        validate_scene_references(&mut scenes);
        validate_control_flow_references(&mut scenes);
        return Ok(scenes);
    }
    let mut merged = BTreeMap::new();
    let source_reader = SourceReader::for_mounts();
    for script_mount in project.script_mounts() {
        for mut scene in load_directory(&script_mount, languages, &source_reader)? {
            if let Some(previous) = merged.insert(scene.name.clone(), scene.clone()) {
                scene.diagnostics.push(Diagnostic {
                    level: DiagnosticLevel::Warning,
                    span: crate::SourceSpan { line: 1, column: 1 },
                    message: format!(
                        "scene {:?} overrides {}",
                        scene.name,
                        previous.path.display()
                    ),
                });
                merged.insert(scene.name.clone(), scene);
            }
        }
    }
    let mut scenes = merged.into_values().collect::<Vec<_>>();
    validate_eiyashou_project_semantics(&mut scenes);
    apply_eiyashou_project(project, &mut scenes);
    validate_scene_references(&mut scenes);
    validate_control_flow_references(&mut scenes);
    validate_native_scene_graph(&mut scenes);
    Ok(scenes)
}

fn validate_eiyashou_project_semantics(scenes: &mut [LoadedScene]) {
    let native_indices = scenes
        .iter()
        .enumerate()
        .filter_map(|(index, scene)| is_native_scene_path(&scene.path).then_some(index))
        .collect::<Vec<_>>();
    let diagnostics = {
        let inputs = native_indices
            .iter()
            .map(|index| {
                let scene = &scenes[*index];
                (scene.actions.as_slice(), scene.action_spans.as_slice())
            })
            .collect::<Vec<_>>();
        eiyashou_semantic_diagnostics(&inputs)
    };
    for (index, diagnostics) in native_indices.into_iter().zip(diagnostics) {
        scenes[index].diagnostics.extend(diagnostics);
    }
}

fn apply_eiyashou_project(project: &ContentProject, scenes: &mut [LoadedScene]) {
    let Some(project_data) = &project.eiyashou else {
        return;
    };
    let mut used = HashSet::<(crate::ResourceKind, String)>::new();
    let mut pending = Vec::<(usize, Diagnostic)>::new();
    for (scene_index, scene) in scenes.iter_mut().enumerate() {
        if !is_native_scene_path(&scene.path) {
            continue;
        }
        for (action_index, action) in scene.actions.iter_mut().enumerate() {
            if let Action::EiyashouSay(dialogue) = action
                && !dialogue.speaker.is_empty()
            {
                match project_data.characters.get(&dialogue.speaker) {
                    Some(character) => {
                        dialogue.speaker.clone_from(&character.name);
                        dialogue.speaker_color = character.color;
                    }
                    None => pending.push((
                        scene_index,
                        Diagnostic {
                            level: DiagnosticLevel::Error,
                            span: scene
                                .action_spans
                                .get(action_index)
                                .copied()
                                .unwrap_or(crate::SourceSpan { line: 1, column: 1 }),
                            message: format!(
                                "undefined character id `{}` in dialogue",
                                dialogue.speaker
                            ),
                        },
                    )),
                }
            }
        }
        for resource in &scene.resources {
            used.insert((resource.kind, resource.path.clone()));
            if !project_data
                .assets
                .get(&resource.kind)
                .is_some_and(|ids| ids.contains(&resource.path))
            {
                pending.push((
                    scene_index,
                    Diagnostic {
                        level: DiagnosticLevel::Error,
                        span: resource.span,
                        message: format!(
                            "undefined {:?} resource id `{}`",
                            resource.kind, resource.path
                        ),
                    },
                ));
            }
        }
    }
    for (scene_index, diagnostic) in pending {
        scenes[scene_index].diagnostics.push(diagnostic);
    }
    let Some(first_native) = scenes
        .iter_mut()
        .find(|scene| is_native_scene_path(&scene.path))
    else {
        return;
    };
    for warning in &project_data.warnings {
        first_native.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            span: crate::SourceSpan { line: 1, column: 1 },
            message: warning.clone(),
        });
    }
    for (kind, ids) in &project_data.assets {
        for id in ids {
            if !used.contains(&(*kind, id.clone())) {
                first_native.diagnostics.push(Diagnostic {
                    level: DiagnosticLevel::Warning,
                    span: crate::SourceSpan { line: 1, column: 1 },
                    message: format!("unused {:?} resource `{id}`", kind),
                });
            }
        }
    }
}

/// Loads startup scenes while allowing a packaged loader to transfer its
/// decoded action tree exactly once. Directory/editor loaders remain reusable
/// for validation and hot reload.
pub fn load_startup_scenes_with(
    project: &ContentProject,
    languages: &ScriptLanguageRegistry,
) -> Result<Vec<LoadedScene>> {
    if let Some(loader) = project.scene_loader() {
        let mut scenes = loader.load_startup(&project.root)?;
        validate_scene_references(&mut scenes);
        validate_control_flow_references(&mut scenes);
        return Ok(scenes);
    }
    load_scenes_with(project, languages)
}

fn load_directory(
    scripts: &ContentMount,
    languages: &ScriptLanguageRegistry,
    source_reader: &SourceReader,
) -> Result<Vec<LoadedScene>> {
    let paths = script_paths(scripts, languages)?;

    let mut scenes = Vec::with_capacity(paths.len());
    for path in paths {
        scenes.extend(load_scene(scripts, path, languages, source_reader)?);
    }
    let mut names = BTreeMap::<String, PathBuf>::new();
    for scene in &mut scenes {
        if let Some(previous) = names.insert(scene.name.clone(), scene.path.clone()) {
            if is_native_scene_path(&scene.path) {
                scene.diagnostics.push(Diagnostic {
                    level: DiagnosticLevel::Error,
                    span: scene
                        .action_spans
                        .first()
                        .copied()
                        .unwrap_or(crate::SourceSpan { line: 1, column: 1 }),
                    message: format!(
                        "duplicate native scene {:?}; first declared in {}",
                        scene.name,
                        previous.display()
                    ),
                });
            } else {
                anyhow::bail!(
                    "duplicate scene name {:?} in {}",
                    scene.name,
                    scripts.prefix().display()
                );
            }
        }
    }
    Ok(scenes)
}

fn validate_native_scene_graph(scenes: &mut [LoadedScene]) {
    let native_names = scenes
        .iter()
        .filter(|scene| is_native_scene_path(&scene.path))
        .map(|scene| scene.name.clone())
        .collect::<HashSet<_>>();
    if native_names.is_empty() {
        return;
    }

    let mut call_edges = Vec::<(String, String, crate::SourceSpan)>::new();
    let mut non_yielding_goto_edges = Vec::<(String, String, crate::SourceSpan)>::new();
    let mut local_cycles = Vec::<(String, crate::SourceSpan)>::new();
    for scene in scenes
        .iter()
        .filter(|scene| native_names.contains(&scene.name))
    {
        if let Some(index) = non_yielding_cycle_index(&scene.actions) {
            local_cycles.push((
                scene.name.clone(),
                scene
                    .action_spans
                    .get(index)
                    .copied()
                    .unwrap_or(crate::SourceSpan { line: 1, column: 1 }),
            ));
        }
        let mut yielded = false;
        for (index, action) in scene.actions.iter().enumerate() {
            let span = scene
                .action_spans
                .get(index)
                .copied()
                .unwrap_or(crate::SourceSpan { line: 1, column: 1 });
            collect_native_call_edges(action, &scene.name, span, &mut call_edges);
            if action_yields(action) {
                yielded = true;
            }
            if let Action::ChangeScene(target) = action {
                if !yielded && native_names.contains(target) {
                    non_yielding_goto_edges.push((scene.name.clone(), target.clone(), span));
                }
                break;
            }
        }
    }

    let recursive_calls = cyclic_edge_indices(&call_edges);
    let spinning_gotos = cyclic_edge_indices(&non_yielding_goto_edges);
    for index in recursive_calls {
        let (source, target, span) = &call_edges[index];
        push_native_graph_error(
            scenes,
            source,
            *span,
            format!("recursive native scene call `{source}` -> `{target}` is not allowed"),
        );
    }
    for index in spinning_gotos {
        let (source, target, span) = &non_yielding_goto_edges[index];
        push_native_graph_error(
            scenes,
            source,
            *span,
            format!(
                "native control-flow cycle `{source}` -> `{target}` can repeat without yielding"
            ),
        );
    }
    for (scene, span) in local_cycles {
        push_native_graph_error(
            scenes,
            &scene,
            span,
            "native control-flow cycle can repeat without yielding".into(),
        );
    }
}

/// Reject an explicit native `return` on any path that can be reached without
/// first entering through `CallScene`. This entry-aware pass lives above the
/// parser because the configured entry belongs to project configuration.
pub fn validate_native_entry_flow(scenes: &mut [LoadedScene], entry: &str) {
    let native_names = scenes
        .iter()
        .filter(|scene| is_native_scene_path(&scene.path))
        .map(|scene| scene.name.clone())
        .collect::<HashSet<_>>();
    if !native_names.contains(entry) {
        return;
    }

    let mut pending = VecDeque::from([(entry.to_owned(), false)]);
    let mut visited = HashSet::new();
    let mut invalid_returns = Vec::new();
    while let Some((name, has_call_frame)) = pending.pop_front() {
        if !visited.insert((name.clone(), has_call_frame)) {
            continue;
        }
        let Some(scene) = scenes.iter().find(|scene| scene.name == name) else {
            continue;
        };
        for (index, action) in scene.actions.iter().enumerate() {
            let action = unwrap_flow(action);
            let span = scene
                .action_spans
                .get(index)
                .copied()
                .unwrap_or(crate::SourceSpan { line: 1, column: 1 });
            match action {
                Action::ReturnScene => {
                    if !has_call_frame {
                        invalid_returns.push((name.clone(), span));
                    }
                    break;
                }
                Action::ChangeScene(target) => {
                    if native_names.contains(target) {
                        pending.push_back((target.clone(), has_call_frame));
                    }
                    break;
                }
                Action::CallScene(target) => {
                    if native_names.contains(target) {
                        pending.push_back((target.clone(), true));
                    }
                }
                Action::Menu { choices, .. } => {
                    let mut can_resume = false;
                    for choice in choices {
                        match &choice.target {
                            ChoiceTarget::ChangeScene(target) => {
                                if native_names.contains(target) {
                                    pending.push_back((target.clone(), has_call_frame));
                                }
                            }
                            ChoiceTarget::CallScene(target) => {
                                if native_names.contains(target) {
                                    pending.push_back((target.clone(), true));
                                }
                                can_resume = true;
                            }
                            ChoiceTarget::Label(_) => can_resume = true,
                        }
                    }
                    if !can_resume {
                        break;
                    }
                }
                Action::EiyashouMenu { choices, .. } => {
                    let mut can_resume = false;
                    for choice in choices {
                        match &choice.target {
                            ChoiceTarget::ChangeScene(target) => {
                                if native_names.contains(target) {
                                    pending.push_back((target.clone(), has_call_frame));
                                }
                            }
                            ChoiceTarget::CallScene(target) => {
                                if native_names.contains(target) {
                                    pending.push_back((target.clone(), true));
                                }
                                can_resume = true;
                            }
                            ChoiceTarget::Label(_) => can_resume = true,
                        }
                    }
                    if !can_resume {
                        break;
                    }
                }
                Action::End => break,
                _ => {}
            }
        }
    }

    for (scene, span) in invalid_returns {
        push_native_graph_error(
            scenes,
            &scene,
            span,
            "native `return` is reachable with an empty call stack".into(),
        );
    }
    validate_eiyashou_project_initialization(scenes, entry, &native_names);
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct FlowContext {
    scene: String,
    action: usize,
    stack: Vec<(String, usize)>,
}

/// Validate global Eiyashou variables from the configured entry across scene
/// changes and the (non-recursive) call stack. Parser-local validation handles
/// branches within one source; this project pass prevents a scene from reading
/// a declaration that exists elsewhere but is not guaranteed on every route.
fn validate_eiyashou_project_initialization(
    scenes: &mut [LoadedScene],
    entry: &str,
    native_names: &HashSet<String>,
) {
    let scene_indices = scenes
        .iter()
        .enumerate()
        .filter(|(_, scene)| native_names.contains(&scene.name))
        .map(|(index, scene)| (scene.name.clone(), index))
        .collect::<HashMap<_, _>>();
    let labels = scenes
        .iter()
        .filter(|scene| native_names.contains(&scene.name))
        .map(|scene| {
            let labels = scene
                .actions
                .iter()
                .enumerate()
                .filter_map(|(index, action)| match unwrap_flow(action) {
                    Action::Label(label) => Some((label.clone(), index)),
                    _ => None,
                })
                .collect::<HashMap<_, _>>();
            (scene.name.clone(), labels)
        })
        .collect::<HashMap<_, _>>();
    let Some(&entry_index) = scene_indices.get(entry) else {
        return;
    };
    if scenes[entry_index].actions.is_empty() {
        return;
    }

    let start = FlowContext {
        scene: entry.to_owned(),
        action: 0,
        stack: Vec::new(),
    };
    let mut incoming = HashMap::from([(start.clone(), HashSet::<String>::new())]);
    let mut pending = VecDeque::from([start]);
    while let Some(context) = pending.pop_front() {
        let initialized = incoming.get(&context).cloned().unwrap_or_default();
        let Some(&scene_index) = scene_indices.get(&context.scene) else {
            continue;
        };
        let Some(action) = scenes[scene_index].actions.get(context.action) else {
            continue;
        };
        let mut after = initialized;
        if let Action::EiyashouSet {
            target: EiyashouPlace::Variable(name),
            initialize_once: true,
            ..
        } = unwrap_flow(action)
        {
            after.insert(name.clone());
        }
        for next in project_successors(&context, action, &labels, native_names.len()) {
            let changed = match incoming.get_mut(&next) {
                Some(existing) => {
                    let intersection = existing.intersection(&after).cloned().collect();
                    if *existing == intersection {
                        false
                    } else {
                        *existing = intersection;
                        true
                    }
                }
                None => {
                    incoming.insert(next.clone(), after.clone());
                    true
                }
            };
            if changed {
                pending.push_back(next);
            }
        }
    }

    let mut errors = HashSet::new();
    for (context, initialized) in incoming {
        let Some(&scene_index) = scene_indices.get(&context.scene) else {
            continue;
        };
        let Some(action) = scenes[scene_index].actions.get(context.action) else {
            continue;
        };
        let mut reads = HashSet::new();
        collect_eiyashou_action_reads(unwrap_flow(action), &mut reads);
        for variable in reads.difference(&initialized) {
            errors.insert((scene_index, context.action, variable.clone()));
        }
    }
    for (scene_index, action_index, variable) in errors {
        let span = scenes[scene_index]
            .action_spans
            .get(action_index)
            .copied()
            .unwrap_or(crate::SourceSpan { line: 1, column: 1 });
        scenes[scene_index].diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Error,
            span,
            message: format!(
                "variable `{variable}` may be uninitialized on a route from configured entry `{entry}`"
            ),
        });
    }
}

fn project_successors(
    context: &FlowContext,
    action: &Action,
    labels: &HashMap<String, HashMap<String, usize>>,
    max_stack: usize,
) -> Vec<FlowContext> {
    let next = || FlowContext {
        scene: context.scene.clone(),
        action: context.action + 1,
        stack: context.stack.clone(),
    };
    let target = |scene: &str, action: usize, stack: Vec<(String, usize)>| FlowContext {
        scene: scene.to_owned(),
        action,
        stack,
    };
    let action = unwrap_flow(action);
    match action {
        Action::Jump(label) => labels
            .get(&context.scene)
            .and_then(|labels| labels.get(label))
            .map(|index| vec![target(&context.scene, *index, context.stack.clone())])
            .unwrap_or_default(),
        Action::EiyashouJumpIf { label, .. } => {
            let mut values = vec![next()];
            if let Some(index) = labels
                .get(&context.scene)
                .and_then(|labels| labels.get(label))
            {
                values.push(target(&context.scene, *index, context.stack.clone()));
            }
            values
        }
        Action::ChangeScene(scene) => vec![target(scene, 0, context.stack.clone())],
        Action::CallScene(scene) if context.stack.len() < max_stack => {
            let mut stack = context.stack.clone();
            stack.push((context.scene.clone(), context.action + 1));
            vec![target(scene, 0, stack)]
        }
        Action::CallScene(_) => Vec::new(),
        Action::ReturnScene => context
            .stack
            .split_last()
            .map(|((scene, action), stack)| vec![target(scene, *action, stack.to_vec())])
            .unwrap_or_default(),
        Action::EiyashouMenu { choices, .. } => choices
            .iter()
            .filter_map(|choice| match &choice.target {
                ChoiceTarget::Label(label) => labels
                    .get(&context.scene)
                    .and_then(|labels| labels.get(label))
                    .map(|index| target(&context.scene, *index, context.stack.clone())),
                ChoiceTarget::ChangeScene(scene) => Some(target(scene, 0, context.stack.clone())),
                ChoiceTarget::CallScene(scene) if context.stack.len() < max_stack => {
                    let mut stack = context.stack.clone();
                    stack.push((context.scene.clone(), context.action + 1));
                    Some(target(scene, 0, stack))
                }
                ChoiceTarget::CallScene(_) => None,
            })
            .collect(),
        Action::End => Vec::new(),
        _ => vec![next()],
    }
}

fn collect_eiyashou_action_reads(action: &Action, reads: &mut HashSet<String>) {
    match action {
        Action::EiyashouSay(dialogue) => collect_eiyashou_text_reads(&dialogue.text, reads),
        Action::EiyashouMenu { prompt, choices } => {
            collect_eiyashou_text_reads(prompt, reads);
            for choice in choices {
                collect_eiyashou_text_reads(&choice.text, reads);
                if let Some(condition) = &choice.show_when {
                    collect_eiyashou_expression_reads(condition, reads);
                }
            }
        }
        Action::EiyashouSet {
            target,
            expression,
            initialize_once,
            ..
        } => {
            collect_eiyashou_expression_reads(expression, reads);
            match target {
                EiyashouPlace::Variable(name) if !initialize_once => {
                    reads.insert(name.clone());
                }
                EiyashouPlace::Index { variable, index } => {
                    reads.insert(variable.clone());
                    collect_eiyashou_expression_reads(index, reads);
                }
                _ => {}
            }
        }
        Action::EiyashouList {
            variable,
            operation,
        } => {
            reads.insert(variable.clone());
            match operation {
                EiyashouListOperation::Append(value) | EiyashouListOperation::Remove(value) => {
                    collect_eiyashou_expression_reads(value, reads);
                }
                EiyashouListOperation::Insert { index, value } => {
                    collect_eiyashou_expression_reads(index, reads);
                    collect_eiyashou_expression_reads(value, reads);
                }
                EiyashouListOperation::Pop { index, into } => {
                    if let Some(index) = index {
                        collect_eiyashou_expression_reads(index, reads);
                    }
                    reads.insert(into.clone());
                }
                EiyashouListOperation::Clear => {}
            }
        }
        Action::EiyashouJumpIf { condition, .. } => {
            collect_eiyashou_expression_reads(condition, reads);
        }
        _ => {}
    }
}

fn collect_eiyashou_text_reads(text: &EiyashouText, reads: &mut HashSet<String>) {
    for part in &text.parts {
        if let EiyashouTextPart::Expression(expression) = part {
            collect_eiyashou_expression_reads(expression, reads);
        }
    }
}

fn collect_eiyashou_expression_reads(expression: &EiyashouExpr, reads: &mut HashSet<String>) {
    match expression {
        EiyashouExpr::Variable(name) => {
            reads.insert(name.clone());
        }
        EiyashouExpr::List(values) => {
            for value in values {
                collect_eiyashou_expression_reads(value, reads);
            }
        }
        EiyashouExpr::Unary { value, .. } | EiyashouExpr::Length(value) => {
            collect_eiyashou_expression_reads(value, reads);
        }
        EiyashouExpr::Binary { left, right, .. } => {
            collect_eiyashou_expression_reads(left, reads);
            collect_eiyashou_expression_reads(right, reads);
        }
        EiyashouExpr::Index { list, index } => {
            collect_eiyashou_expression_reads(list, reads);
            collect_eiyashou_expression_reads(index, reads);
        }
        EiyashouExpr::Literal(_) | EiyashouExpr::EmptyList(_) => {}
    }
}

fn unwrap_flow(mut action: &Action) -> &Action {
    while let Action::Flow { action: inner, .. } = action {
        action = inner;
    }
    action
}

fn is_native_scene_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("shou"))
}

fn collect_native_call_edges(
    action: &Action,
    source: &str,
    span: crate::SourceSpan,
    edges: &mut Vec<(String, String, crate::SourceSpan)>,
) {
    match action {
        Action::CallScene(target) => edges.push((source.to_owned(), target.clone(), span)),
        Action::Menu { choices, .. } => {
            for choice in choices {
                if let ChoiceTarget::CallScene(target) = &choice.target {
                    edges.push((source.to_owned(), target.clone(), span));
                }
            }
        }
        Action::EiyashouMenu { choices, .. } => {
            for choice in choices {
                if let ChoiceTarget::CallScene(target) = &choice.target {
                    edges.push((source.to_owned(), target.clone(), span));
                }
            }
        }
        Action::Flow { action, .. } => collect_native_call_edges(action, source, span, edges),
        _ => {}
    }
}

fn action_yields(action: &Action) -> bool {
    match action {
        Action::Say { .. }
        | Action::Menu { .. }
        | Action::EiyashouSay(_)
        | Action::EiyashouMenu { .. } => true,
        Action::Wait { seconds } => *seconds > 0.0,
        Action::PlayVideo { video } => video.wait_for_finished,
        Action::MoveSprite {
            duration, blocking, ..
        } => *blocking && *duration > 0.0,
        Action::Flow { action, .. } => action_yields(action),
        _ => false,
    }
}

fn non_yielding_cycle_index(actions: &[Action]) -> Option<usize> {
    let labels = actions
        .iter()
        .enumerate()
        .filter_map(|(index, action)| match action {
            Action::Label(label) => Some((label.as_str(), index)),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let mut colors = vec![0u8; actions.len()];
    fn visit(
        index: usize,
        actions: &[Action],
        labels: &BTreeMap<&str, usize>,
        colors: &mut [u8],
    ) -> Option<usize> {
        if action_yields(&actions[index]) {
            return None;
        }
        match colors[index] {
            1 => return Some(index),
            2 => return None,
            _ => {}
        }
        colors[index] = 1;
        for successor in native_action_successors(index, actions, labels) {
            if let Some(cycle) = visit(successor, actions, labels, colors) {
                return Some(cycle);
            }
        }
        colors[index] = 2;
        None
    }
    for index in 0..actions.len() {
        if let Some(cycle) = visit(index, actions, &labels, &mut colors) {
            return Some(cycle);
        }
    }
    None
}

fn native_action_successors(
    index: usize,
    actions: &[Action],
    labels: &BTreeMap<&str, usize>,
) -> Vec<usize> {
    let next = (index + 1 < actions.len()).then_some(index + 1);
    match &actions[index] {
        Action::Jump(label) => labels.get(label.as_str()).copied().into_iter().collect(),
        Action::EiyashouJumpIf { label, .. } => labels
            .get(label.as_str())
            .copied()
            .into_iter()
            .chain(next)
            .collect(),
        Action::Menu { choices, .. } => choices
            .iter()
            .filter_map(|choice| match &choice.target {
                ChoiceTarget::Label(label) => labels.get(label.as_str()).copied(),
                _ => None,
            })
            .collect(),
        Action::EiyashouMenu { choices, .. } => choices
            .iter()
            .filter_map(|choice| match &choice.target {
                ChoiceTarget::Label(label) => labels.get(label.as_str()).copied(),
                _ => None,
            })
            .collect(),
        Action::ChangeScene(_) | Action::ReturnScene | Action::End => Vec::new(),
        _ => next.into_iter().collect(),
    }
}

fn cyclic_edge_indices(edges: &[(String, String, crate::SourceSpan)]) -> Vec<usize> {
    edges
        .iter()
        .enumerate()
        .filter_map(|(index, (source, target, _))| {
            let mut pending = vec![target.as_str()];
            let mut visited = HashSet::new();
            while let Some(node) = pending.pop() {
                if node == source {
                    return Some(index);
                }
                if visited.insert(node) {
                    pending.extend(
                        edges
                            .iter()
                            .filter(|(candidate, _, _)| candidate == node)
                            .map(|(_, next, _)| next.as_str()),
                    );
                }
            }
            None
        })
        .collect()
}

fn push_native_graph_error(
    scenes: &mut [LoadedScene],
    source: &str,
    span: crate::SourceSpan,
    message: String,
) {
    if let Some(scene) = scenes.iter_mut().find(|scene| scene.name == source) {
        scene.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Error,
            span,
            message,
        });
    }
}

fn script_paths(
    scripts: &ContentMount,
    languages: &ScriptLanguageRegistry,
) -> Result<Vec<PathBuf>> {
    let mut paths = scripts.recursive_files()?;
    paths.retain(|path| languages.supports(path));
    paths.sort();
    Ok(paths)
}

fn validate_scene_references(scenes: &mut [LoadedScene]) {
    let names = scenes
        .iter()
        .map(|scene| scene.name.clone())
        .collect::<HashSet<_>>();
    for scene in scenes {
        for reference in &scene.sub_scenes {
            if !reference.scene.contains('{') && !names.contains(&reference.scene) {
                scene.diagnostics.push(Diagnostic {
                    level: DiagnosticLevel::Error,
                    span: reference.span,
                    message: format!("referenced scene {:?} does not exist", reference.scene),
                });
            }
        }
    }
}

fn validate_control_flow_references(scenes: &mut [LoadedScene]) {
    for scene in scenes {
        let mut labels = HashSet::new();
        let mut diagnostics = Vec::new();
        for (index, action) in scene.actions.iter().enumerate() {
            let span = scene
                .action_spans
                .get(index)
                .copied()
                .unwrap_or(crate::SourceSpan { line: 1, column: 1 });
            register_labels(action, span, &mut labels, &mut diagnostics);
        }
        for (index, action) in scene.actions.iter().enumerate() {
            let span = scene
                .action_spans
                .get(index)
                .copied()
                .unwrap_or(crate::SourceSpan { line: 1, column: 1 });
            validate_label_references(action, span, &labels, &mut diagnostics);
        }
        scene.diagnostics.extend(diagnostics);
    }
}

fn register_labels(
    action: &Action,
    span: crate::SourceSpan,
    labels: &mut HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match action {
        Action::Label(label) => {
            if !labels.insert(label.clone()) {
                diagnostics.push(Diagnostic {
                    level: DiagnosticLevel::Error,
                    span,
                    message: format!("duplicate label {label:?}"),
                });
            }
        }
        Action::Flow { action, .. } => register_labels(action, span, labels, diagnostics),
        _ => {}
    }
}

fn validate_label_references(
    action: &Action,
    span: crate::SourceSpan,
    labels: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut validate = |label: &str| {
        if !label.contains('{') && !labels.contains(label) {
            diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Error,
                span,
                message: format!("referenced label {label:?} does not exist"),
            });
        }
    };
    match action {
        Action::Jump(label) => validate(label),
        Action::Menu { choices, .. } => {
            for choice in choices {
                if let ChoiceTarget::Label(label) = &choice.target {
                    validate(label);
                }
            }
        }
        Action::EiyashouMenu { choices, .. } => {
            for choice in choices {
                if let ChoiceTarget::Label(label) = &choice.target {
                    validate(label);
                }
            }
        }
        Action::EiyashouJumpIf { label, .. } => validate(label),
        Action::Flow { action, .. } => {
            validate_label_references(action, span, labels, diagnostics);
        }
        _ => {}
    }
}

fn load_scene(
    scripts: &ContentMount,
    path: PathBuf,
    languages: &ScriptLanguageRegistry,
    source_reader: &SourceReader,
) -> Result<Vec<LoadedScene>> {
    let language = languages
        .language_for(&path)
        .with_context(|| format!("unsupported script format: {}", path.display()))?;
    let bytes = source_reader
        .read_mount(scripts, &path)
        .with_context(|| format!("failed to read script {}", path.display()))?;
    let source = String::from_utf8(bytes)
        .with_context(|| format!("script is not UTF-8: {}", path.display()))?;
    let mut name_path = path.clone();
    name_path.set_extension("");
    let name = name_path
        .to_str()
        .with_context(|| format!("script has no valid UTF-8 path: {}", path.display()))?
        .replace('\\', "/");

    Ok(language
        .parse_scenes(&source)
        .into_iter()
        .map(|parsed| {
            let report = parsed.report;
            LoadedScene {
                name: parsed.name.unwrap_or_else(|| name.clone()),
                path: path.clone(),
                actions: report.actions,
                action_spans: report.spans,
                diagnostics: report.diagnostics,
                resources: report.resources,
                sub_scenes: report.sub_scenes,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use keine_core::{Program, State, StepResult, step};

    fn project(root: &Path) -> ContentProject {
        ContentProject {
            root: root.to_owned(),
            sources: vec![crate::SourceMount::project("project", root.to_owned())],
            scene_loader: None,
            eiyashou: None,
        }
    }

    #[test]
    fn detects_supported_languages() {
        let languages = ScriptLanguageRegistry::default();
        assert!(languages.supports(Path::new("scene.txt")));
        assert!(languages.supports(Path::new("scene.shou")));
        assert!(!languages.supports(Path::new("scene.md")));
    }

    #[test]
    fn loads_scenes_in_filename_order() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project_root = std::env::temp_dir().join(format!("keine-scenes-{nonce}"));
        let root = project_root.join("scripts");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("b.txt"), "B:second;").unwrap();
        fs::write(root.join("a.txt"), "A:first;").unwrap();
        fs::write(root.join("ignored.md"), "not a scene").unwrap();

        let scenes = load_scenes(&project(&project_root)).unwrap();

        assert_eq!(
            scenes
                .iter()
                .map(|scene| scene.name.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        let _ = fs::remove_dir_all(project_root);
    }

    #[test]
    fn loads_multiple_native_scenes_from_one_source_file() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project_root = std::env::temp_dir().join(format!("keine-native-scenes-{nonce}"));
        let root = project_root.join("scripts");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("chapter.shou"),
            r#"
scene opening {
  "Start",
  goto(ending)
}

scene ending {
  video(credits, skippable: false),
  "End"
}
"#,
        )
        .unwrap();

        let scenes = load_scenes(&project(&project_root)).unwrap();

        assert_eq!(
            scenes
                .iter()
                .map(|scene| scene.name.as_str())
                .collect::<Vec<_>>(),
            ["ending", "opening"]
        );
        assert!(scenes.iter().all(|scene| scene.diagnostics.is_empty()));
        assert!(
            scenes
                .iter()
                .find(|scene| scene.name == "opening")
                .unwrap()
                .sub_scenes
                .iter()
                .any(|reference| reference.scene == "ending")
        );
        assert!(
            scenes
                .iter()
                .find(|scene| scene.name == "ending")
                .unwrap()
                .resources
                .iter()
                .any(|resource| resource.path == "credits")
        );
        let _ = fs::remove_dir_all(project_root);
    }

    #[test]
    fn native_variables_are_typed_across_source_files() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project_root = std::env::temp_dir().join(format!("keine-native-types-{nonce}"));
        let root = project_root.join("scripts");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("opening.shou"),
            r#"scene opening { let visits = 1, goto(ending) }"#,
        )
        .unwrap();
        fs::write(
            root.join("ending.shou"),
            r#"scene ending { visits += 1, "Visits: ${visits}" }"#,
        )
        .unwrap();

        let scenes = load_scenes(&project(&project_root)).unwrap();

        assert!(
            scenes
                .iter()
                .flat_map(|scene| &scene.diagnostics)
                .all(|diagnostic| !diagnostic.message.contains("unknown variable"))
        );
        let _ = fs::remove_dir_all(project_root);
    }

    #[test]
    fn native_scene_graph_rejects_recursive_calls_and_non_yielding_goto_cycles() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project_root = std::env::temp_dir().join(format!("keine-native-flow-{nonce}"));
        let root = project_root.join("scripts");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("flow.shou"),
            r#"
scene call_a { call(call_b) }
scene call_b { call(call_a) }
scene spin_a { goto(spin_b) }
scene spin_b { goto(spin_a) }
scene allowed_a { "yield", goto(allowed_b) }
scene allowed_b { goto(allowed_a) }
"#,
        )
        .unwrap();

        let scenes = load_scenes(&project(&project_root)).unwrap();
        let messages = scenes
            .iter()
            .flat_map(|scene| scene.diagnostics.iter())
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>();

        assert!(
            messages
                .iter()
                .any(|message| message.contains("recursive native scene call"))
        );
        assert!(
            messages
                .iter()
                .any(|message| message.contains("can repeat without yielding"))
        );
        assert!(
            !scenes
                .iter()
                .find(|scene| scene.name == "allowed_a")
                .unwrap()
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("without yielding"))
        );
        let _ = fs::remove_dir_all(project_root);
    }

    #[test]
    fn native_entry_flow_rejects_only_returns_reachable_without_call_frames() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project_root = std::env::temp_dir().join(format!("keine-native-return-{nonce}"));
        let root = project_root.join("scripts");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("flow.shou"),
            r#"
scene start { call(legal), goto(empty_path) }
scene legal { return }
scene empty_path { return }
"#,
        )
        .unwrap();

        let mut scenes = load_scenes(&project(&project_root)).unwrap();
        validate_native_entry_flow(&mut scenes, "start");

        assert!(
            scenes
                .iter()
                .find(|scene| scene.name == "legal")
                .unwrap()
                .diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.message.contains("empty call stack"))
        );
        assert!(
            scenes
                .iter()
                .find(|scene| scene.name == "empty_path")
                .unwrap()
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("empty call stack"))
        );
        let _ = fs::remove_dir_all(project_root);
    }

    #[test]
    fn native_entry_flow_checks_initialization_across_scene_routes() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project_root = std::env::temp_dir().join(format!("keine-native-init-{nonce}"));
        let root = project_root.join("scripts");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("flow.shou"),
            r#"
scene start {
  choice("route") {
    "without": goto(read_without_init),
    "with": goto(setup)
  }
}
scene setup { let name = "Rin", goto(read_after_init) }
scene read_without_init { "${name}" }
scene read_after_init { "${name}" }
"#,
        )
        .unwrap();

        let mut scenes = load_scenes(&project(&project_root)).unwrap();
        validate_native_entry_flow(&mut scenes, "start");

        assert!(
            scenes
                .iter()
                .find(|scene| scene.name == "read_without_init")
                .unwrap()
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("route from configured entry"))
        );
        assert!(
            scenes
                .iter()
                .find(|scene| scene.name == "read_after_init")
                .unwrap()
                .diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.message.contains("route from configured entry"))
        );
        let _ = fs::remove_dir_all(project_root);
    }

    #[test]
    fn loaded_scenes_execute_call_and_return_end_to_end() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project_root = std::env::temp_dir().join(format!("keine-scene-flow-{nonce}"));
        let root = project_root.join("scripts");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("main.txt"),
            "callScene:aside.txt;\nMain:returned;",
        )
        .unwrap();
        fs::write(root.join("aside.txt"), "Aside: inside;").unwrap();

        let mut state = State::new();
        state.install_program(Program::from_scenes(
            load_scenes(&project(&project_root))
                .unwrap()
                .into_iter()
                .map(|scene| (scene.name, scene.actions)),
        ));
        state.current_scene = "main".into();

        assert_eq!(step::step(&mut state), StepResult::AwaitClick);
        assert_eq!(state.current_scene, "aside");
        assert_eq!(state.dialogue.as_ref().unwrap().text, "inside");
        step::advance(&mut state);
        assert_eq!(step::step(&mut state), StepResult::AwaitClick);
        assert_eq!(state.current_scene, "main");
        assert_eq!(state.dialogue.as_ref().unwrap().text, "returned");

        let _ = fs::remove_dir_all(project_root);
    }

    #[test]
    fn recursively_loads_and_executes_deeply_nested_scene_paths() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project_root = std::env::temp_dir().join(format!("keine-nested-scenes-{nonce}"));
        let root = project_root.join("scripts");
        let nested_scene = "chapter_01/act_02/branch_03/part_04";
        fs::create_dir_all(root.join("chapter_01/act_02/branch_03")).unwrap();
        fs::write(
            root.join("main.txt"),
            format!("callScene:{nested_scene}.txt;\nMain:returned;"),
        )
        .unwrap();
        fs::write(
            root.join(format!("{nested_scene}.txt")),
            "Guide:inside nested scene;",
        )
        .unwrap();

        let scenes = load_scenes(&project(&project_root)).unwrap();
        assert_eq!(
            scenes
                .iter()
                .map(|scene| scene.name.as_str())
                .collect::<Vec<_>>(),
            [nested_scene, "main"]
        );
        assert!(scenes.iter().all(|scene| scene.diagnostics.is_empty()));

        let mut state = State::new();
        state.install_program(Program::from_scenes(
            scenes.into_iter().map(|scene| (scene.name, scene.actions)),
        ));
        state.current_scene = "main".into();

        assert_eq!(step::step(&mut state), StepResult::AwaitClick);
        assert_eq!(state.current_scene, nested_scene);
        assert_eq!(state.dialogue.as_ref().unwrap().text, "inside nested scene");
        step::advance(&mut state);
        assert_eq!(step::step(&mut state), StepResult::AwaitClick);
        assert_eq!(state.current_scene, "main");
        assert_eq!(state.dialogue.as_ref().unwrap().text, "returned");

        let _ = fs::remove_dir_all(project_root);
    }

    #[cfg(unix)]
    #[test]
    fn filesystem_scan_ignores_symlink_cycles_and_mount_escapes() {
        use std::os::unix::fs::symlink;

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project_root = std::env::temp_dir().join(format!("keine-scene-links-{nonce}"));
        let outside_root = std::env::temp_dir().join(format!("keine-scene-links-outside-{nonce}"));
        let scripts = project_root.join("scripts");
        let nested = scripts.join("chapter");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(&outside_root).unwrap();
        fs::write(nested.join("inside.txt"), "Guide:inside mount;").unwrap();
        fs::write(outside_root.join("outside.txt"), "Intruder:outside mount;").unwrap();

        symlink(&scripts, nested.join("cycle")).unwrap();
        symlink(&outside_root, scripts.join("escaped-directory")).unwrap();
        symlink(
            outside_root.join("outside.txt"),
            scripts.join("escaped-file.txt"),
        )
        .unwrap();

        let scenes = load_scenes(&project(&project_root)).unwrap();

        assert_eq!(
            scenes
                .iter()
                .map(|scene| scene.name.as_str())
                .collect::<Vec<_>>(),
            ["chapter/inside"]
        );

        let _ = fs::remove_dir_all(project_root);
        let _ = fs::remove_dir_all(outside_root);
    }

    #[test]
    fn same_stem_in_different_directories_has_distinct_scene_names() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project_root = std::env::temp_dir().join(format!("keine-scene-stems-{nonce}"));
        let root = project_root.join("scripts");
        fs::create_dir_all(root.join("chapter_a")).unwrap();
        fs::create_dir_all(root.join("chapter_b")).unwrap();
        fs::write(root.join("chapter_a/part.txt"), "A:first;").unwrap();
        fs::write(root.join("chapter_b/part.txt"), "B:second;").unwrap();

        let scenes = load_scenes(&project(&project_root)).unwrap();
        assert_eq!(
            scenes
                .iter()
                .map(|scene| scene.name.as_str())
                .collect::<Vec<_>>(),
            ["chapter_a/part", "chapter_b/part"]
        );

        let _ = fs::remove_dir_all(project_root);
    }

    #[test]
    fn later_script_sources_override_earlier_scenes() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-scene-layers-{nonce}"));
        let base_project = root.join("base-project");
        let patch_project = root.join("patch-project");
        let base = base_project.join("scripts");
        let patch = patch_project.join("scripts");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(&patch).unwrap();
        fs::write(base.join("main.txt"), "Base:old;").unwrap();
        fs::write(patch.join("main.txt"), "Patch:new;").unwrap();
        let project = ContentProject {
            root: root.clone(),
            sources: vec![
                crate::SourceMount::project("project", base_project),
                crate::SourceMount::project("project", patch_project),
            ],
            scene_loader: None,
            eiyashou: None,
        };

        let scenes = load_scenes(&project).unwrap();
        assert_eq!(scenes.len(), 1);
        assert!(
            scenes[0]
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("overrides"))
        );
        let keine_core::Action::Say { text, .. } = &scenes[0].actions[0] else {
            panic!("expected dialogue");
        };
        assert_eq!(text, "new");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn validates_static_labels_recursively_with_source_spans() {
        let actions = vec![
            Action::Label("known".into()),
            Action::Label("known".into()),
            Action::Jump("missing-jump".into()),
            Action::Menu {
                prompt: String::new(),
                choices: vec![keine_core::action::Choice {
                    text: "Missing".into(),
                    target: ChoiceTarget::Label("missing-choice".into()),
                    show_when: None,
                    enable_when: None,
                }],
            },
            Action::Flow {
                action: Box::new(Action::Jump("missing-wrapped".into())),
                when: Some("true".into()),
                next: false,
            },
            Action::Flow {
                action: Box::new(Action::Label("wrapped".into())),
                when: None,
                next: false,
            },
            Action::Jump("wrapped".into()),
            Action::Jump("{dynamic}".into()),
        ];
        let mut scenes = vec![LoadedScene {
            name: "main".into(),
            path: "main.txt".into(),
            action_spans: (1..=actions.len())
                .map(|line| crate::SourceSpan { line, column: 1 })
                .collect(),
            actions,
            diagnostics: Vec::new(),
            resources: Vec::new(),
            sub_scenes: Vec::new(),
        }];

        validate_control_flow_references(&mut scenes);

        let errors = scenes[0]
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.level == DiagnosticLevel::Error)
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 4);
        assert_eq!(errors[0].span.line, 2);
        assert!(errors[0].message.contains("duplicate label"));
        assert_eq!(errors[1].span.line, 3);
        assert!(errors[1].message.contains("missing-jump"));
        assert_eq!(errors[2].span.line, 4);
        assert!(errors[2].message.contains("missing-choice"));
        assert_eq!(errors[3].span.line, 5);
        assert!(errors[3].message.contains("missing-wrapped"));
    }
}
