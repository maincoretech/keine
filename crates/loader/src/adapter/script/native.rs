//! Source-preserving parser and lowering boundary for keine's native DSL.
//!
//! The lexer keeps every byte range, including whitespace and comments. The
//! lowering pass deliberately accepts only semantics that can be represented
//! exactly by today's typed core IR; accepted syntax is never routed through
//! the permissive WebGAL expression evaluator.

use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Range;

use keine_core::{
    Action, BlendMode, ChoiceTarget, Easing, EiyashouAssignOp, EiyashouBinaryOp, EiyashouChoice,
    EiyashouDialogue, EiyashouExpr, EiyashouListOperation, EiyashouPlace, EiyashouScalarType,
    EiyashouText, EiyashouTextPart, EiyashouType, EiyashouUnaryOp, Position, SayOptions,
    SpriteLayout, SpriteTransform, Transition, Value, VideoMode, VideoSpec,
};

use crate::{Diagnostic, DiagnosticLevel, ParseReport, ParsedScene, ScriptLanguage, SourceSpan};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeTokenKind {
    Identifier,
    Number,
    String,
    Punctuation,
    Operator,
    Whitespace,
    Comment,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeToken {
    pub kind: NativeTokenKind,
    pub range: Range<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeSceneSyntax {
    pub name: String,
    /// Exact UTF-8 byte range of the scene identifier. Editor projections may
    /// replace this range without reconstructing the surrounding source.
    pub name_range: Range<usize>,
    pub range: Range<usize>,
}

/// Lossless syntax inventory used by editor-facing consumers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeDocument {
    pub tokens: Vec<NativeToken>,
    pub scenes: Vec<NativeSceneSyntax>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeLanguage;

impl ScriptLanguage for NativeLanguage {
    fn name(&self) -> &'static str {
        "keine"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["shou"]
    }

    fn parse(&self, source: &str) -> ParseReport {
        let mut scenes = compile_native(source, true).1;
        if scenes.len() == 1 {
            return scenes.remove(0).report;
        }
        let mut report = ParseReport::default();
        report.diagnostics.push(error_at(
            source,
            0,
            "native sources are multi-scene; use ScriptLanguage::parse_scenes",
        ));
        report
    }

    fn parse_scenes(&self, source: &str) -> Vec<ParsedScene> {
        // Project loading validates the merged native scene set once, after
        // mount overrides have been resolved. That is the only scope in which
        // global Eiyashou declarations can be typed correctly across files.
        let (_, scenes) = compile_native(source, false);
        scenes
    }
}

pub fn parse_native_document(source: &str) -> NativeDocument {
    // A lossless editor document has no project declaration scope. Keep its
    // diagnostics lexical/syntactic; project validation supplies global type
    // and flow diagnostics after every `.shou` source has been loaded.
    compile_native(source, false).0
}

pub fn parse_native_scenes(source: &str) -> Vec<ParsedScene> {
    compile_native(source, true).1
}

fn compile_native(source: &str, validate_semantics: bool) -> (NativeDocument, Vec<ParsedScene>) {
    let (tokens, lex_diagnostics) = lex(source);
    let mut parser = Parser::new(source, &tokens, lex_diagnostics);
    let mut scenes = parser.parse_file();
    if validate_semantics {
        validate_eiyashou_semantics(&mut scenes);
    }
    let scene_syntax = std::mem::take(&mut parser.scene_syntax);
    let mut diagnostics = std::mem::take(&mut parser.document_diagnostics);
    for diagnostic in scenes
        .iter()
        .flat_map(|scene| scene.report.diagnostics.iter())
    {
        if !diagnostics.contains(diagnostic) {
            diagnostics.push(diagnostic.clone());
        }
    }
    drop(parser);
    let document = NativeDocument {
        tokens,
        scenes: scene_syntax,
        diagnostics,
    };
    (document, scenes)
}

fn validate_eiyashou_semantics(scenes: &mut [ParsedScene]) {
    let diagnostics = {
        let inputs = scenes
            .iter()
            .map(|scene| {
                (
                    scene.report.actions.as_slice(),
                    scene.report.spans.as_slice(),
                )
            })
            .collect::<Vec<_>>();
        eiyashou_semantic_diagnostics(&inputs)
    };
    for (scene, diagnostics) in scenes.iter_mut().zip(diagnostics) {
        scene.report.diagnostics.extend(diagnostics);
    }
}

/// Analyze one complete Eiyashou declaration scope. Editor callers pass one
/// document; project loading passes every native scene after mount overrides
/// have been resolved, so global variables and stable IDs keep project-wide
/// semantics without teaching the runtime about source files.
pub(crate) fn eiyashou_semantic_diagnostics(
    scenes: &[(&[Action], &[SourceSpan])],
) -> Vec<Vec<Diagnostic>> {
    let mut diagnostics = vec![Vec::new(); scenes.len()];
    let mut declarations = HashMap::<String, (usize, usize, EiyashouExpr)>::new();
    let mut types = HashMap::<String, EiyashouType>::new();
    let mut source_ids = HashMap::<String, (usize, usize)>::new();

    for (scene_index, (actions, _)) in scenes.iter().enumerate() {
        for (action_index, action) in actions.iter().enumerate() {
            let source_id = match action {
                Action::EiyashouSay(dialogue) => Some(&dialogue.source_id),
                Action::EiyashouMenu { choices, .. } => {
                    for choice in choices {
                        register_source_id(
                            &mut source_ids,
                            &choice.source_id,
                            scene_index,
                            action_index,
                            scenes,
                            &mut diagnostics,
                        );
                    }
                    None
                }
                _ => None,
            };
            if let Some(source_id) = source_id {
                register_source_id(
                    &mut source_ids,
                    source_id,
                    scene_index,
                    action_index,
                    scenes,
                    &mut diagnostics,
                );
            }
            if let Action::EiyashouSet {
                target: EiyashouPlace::Variable(name),
                expression,
                initialize_once: true,
                ..
            } = action
                && declarations
                    .insert(
                        name.clone(),
                        (scene_index, action_index, expression.clone()),
                    )
                    .is_some()
            {
                push_semantic_error(
                    scenes,
                    &mut diagnostics,
                    scene_index,
                    action_index,
                    format!("variable `{name}` has more than one `let` declaration"),
                );
            }
        }
    }

    let mut remaining = declarations.keys().cloned().collect::<HashSet<_>>();
    loop {
        let before = remaining.len();
        for name in remaining.clone() {
            let (_, _, expression) = &declarations[&name];
            if let Ok(value_type) = infer_expression_type(expression, &types) {
                types.insert(name.clone(), value_type);
                remaining.remove(&name);
            }
        }
        if remaining.is_empty() || remaining.len() == before {
            break;
        }
    }
    for name in remaining {
        let (scene_index, action_index, expression) = &declarations[&name];
        let message = infer_expression_type(expression, &types)
            .err()
            .unwrap_or_else(|| "could not infer declaration type".into());
        push_semantic_error(
            scenes,
            &mut diagnostics,
            *scene_index,
            *action_index,
            format!("invalid declaration `{name}`: {message}"),
        );
    }

    for (scene_index, (actions, _)) in scenes.iter().enumerate() {
        for (action_index, action) in actions.iter().enumerate() {
            if let Err(message) = validate_action_types(action, &types) {
                push_semantic_error(scenes, &mut diagnostics, scene_index, action_index, message);
            }
        }
        for (action_index, message) in definite_initialization_errors(actions, &declarations) {
            push_semantic_error(scenes, &mut diagnostics, scene_index, action_index, message);
        }
    }
    diagnostics
}

fn register_source_id(
    source_ids: &mut HashMap<String, (usize, usize)>,
    source_id: &str,
    scene_index: usize,
    action_index: usize,
    scenes: &[(&[Action], &[SourceSpan])],
    diagnostics: &mut [Vec<Diagnostic>],
) {
    if source_ids
        .insert(source_id.to_owned(), (scene_index, action_index))
        .is_some()
    {
        push_semantic_error(
            scenes,
            diagnostics,
            scene_index,
            action_index,
            format!("duplicate stable source id `{source_id}`"),
        );
    }
}

fn push_semantic_error(
    scenes: &[(&[Action], &[SourceSpan])],
    diagnostics: &mut [Vec<Diagnostic>],
    scene_index: usize,
    action_index: usize,
    message: impl Into<String>,
) {
    diagnostics[scene_index].push(Diagnostic {
        level: DiagnosticLevel::Error,
        span: scenes[scene_index]
            .1
            .get(action_index)
            .copied()
            .unwrap_or(SourceSpan { line: 1, column: 1 }),
        message: message.into(),
    });
}

fn infer_expression_type(
    expression: &EiyashouExpr,
    variables: &HashMap<String, EiyashouType>,
) -> Result<EiyashouType, String> {
    use EiyashouBinaryOp as Binary;
    use EiyashouScalarType as Scalar;
    use EiyashouType as Type;
    match expression {
        EiyashouExpr::Literal(value) => match value {
            Value::Bool(_) => Ok(Type::Scalar(Scalar::Bool)),
            Value::Int(_) => Ok(Type::Scalar(Scalar::Int)),
            Value::Float(_) => Ok(Type::Scalar(Scalar::Float)),
            Value::Str(_) => Ok(Type::Scalar(Scalar::String)),
            Value::Array(values) => infer_value_list_type(values),
        },
        EiyashouExpr::Variable(name) => variables
            .get(name)
            .cloned()
            .ok_or_else(|| format!("unknown variable `{name}`")),
        EiyashouExpr::List(values) => {
            let mut scalar = None;
            for value in values {
                let Type::Scalar(next) = infer_expression_type(value, variables)? else {
                    return Err("nested lists are not allowed".into());
                };
                scalar = Some(merge_scalar_types(scalar, next)?);
            }
            scalar
                .map(Type::List)
                .ok_or_else(|| "empty list literals require `list(type)`".into())
        }
        EiyashouExpr::EmptyList(scalar) => Ok(Type::List(*scalar)),
        EiyashouExpr::Unary { op, value } => {
            let value = infer_expression_type(value, variables)?;
            match (op, value) {
                (EiyashouUnaryOp::Not, Type::Scalar(Scalar::Bool)) => {
                    Ok(Type::Scalar(Scalar::Bool))
                }
                (EiyashouUnaryOp::Negate, Type::Scalar(Scalar::Int)) => {
                    Ok(Type::Scalar(Scalar::Int))
                }
                (EiyashouUnaryOp::Negate, Type::Scalar(Scalar::Float)) => {
                    Ok(Type::Scalar(Scalar::Float))
                }
                _ => Err("invalid unary operand type".into()),
            }
        }
        EiyashouExpr::Binary { op, left, right } => {
            let left = infer_expression_type(left, variables)?;
            let right = infer_expression_type(right, variables)?;
            match op {
                Binary::And | Binary::Or => {
                    require_types(&left, &right, &Type::Scalar(Scalar::Bool))?;
                    Ok(Type::Scalar(Scalar::Bool))
                }
                Binary::Add if left == Type::Scalar(Scalar::String) => {
                    require_type(&right, &Type::Scalar(Scalar::String))?;
                    Ok(Type::Scalar(Scalar::String))
                }
                Binary::Add | Binary::Subtract | Binary::Multiply => {
                    numeric_result(&left, &right, false)
                }
                Binary::Divide => numeric_result(&left, &right, true),
                Binary::Remainder => {
                    require_types(&left, &right, &Type::Scalar(Scalar::Int))?;
                    Ok(Type::Scalar(Scalar::Int))
                }
                Binary::Equal | Binary::NotEqual => {
                    if types_compatible(&left, &right) {
                        Ok(Type::Scalar(Scalar::Bool))
                    } else {
                        Err("equality operands have incompatible types".into())
                    }
                }
                Binary::Less | Binary::LessEqual | Binary::Greater | Binary::GreaterEqual => {
                    numeric_result(&left, &right, false)?;
                    Ok(Type::Scalar(Scalar::Bool))
                }
                Binary::In => match right {
                    Type::List(item) if types_compatible(&left, &Type::Scalar(item)) => {
                        Ok(Type::Scalar(Scalar::Bool))
                    }
                    Type::List(_) => {
                        Err("membership value does not match list element type".into())
                    }
                    _ => Err("right side of `in` must be a list".into()),
                },
            }
        }
        EiyashouExpr::Index { list, index } => {
            require_type(
                &infer_expression_type(index, variables)?,
                &Type::Scalar(Scalar::Int),
            )?;
            if constant_int(index).is_some_and(|value| value < 0) {
                return Err("list indices must not be negative".into());
            }
            if let (EiyashouExpr::List(values), Some(index)) = (list.as_ref(), constant_int(index))
                && usize::try_from(index).is_ok_and(|index| index >= values.len())
            {
                return Err("list index is statically out of bounds".into());
            }
            match infer_expression_type(list, variables)? {
                Type::List(item) => Ok(Type::Scalar(item)),
                _ => Err("only lists can be indexed".into()),
            }
        }
        EiyashouExpr::Length(list) => match infer_expression_type(list, variables)? {
            Type::List(_) => Ok(Type::Scalar(Scalar::Int)),
            _ => Err("`.length` is only valid on a list".into()),
        },
    }
}

fn infer_value_list_type(values: &[Value]) -> Result<EiyashouType, String> {
    let expressions = values
        .iter()
        .cloned()
        .map(EiyashouExpr::Literal)
        .collect::<Vec<_>>();
    infer_expression_type(&EiyashouExpr::List(expressions), &HashMap::new())
}

fn merge_scalar_types(
    current: Option<EiyashouScalarType>,
    next: EiyashouScalarType,
) -> Result<EiyashouScalarType, String> {
    match current {
        None => Ok(next),
        Some(current) if current == next => Ok(current),
        Some(EiyashouScalarType::Int) if next == EiyashouScalarType::Float => {
            Ok(EiyashouScalarType::Float)
        }
        Some(EiyashouScalarType::Float) if next == EiyashouScalarType::Int => {
            Ok(EiyashouScalarType::Float)
        }
        _ => Err("list elements must have one scalar type".into()),
    }
}

fn numeric_result(
    left: &EiyashouType,
    right: &EiyashouType,
    divide: bool,
) -> Result<EiyashouType, String> {
    use EiyashouScalarType::{Float, Int};
    use EiyashouType::Scalar;
    match (left, right) {
        (Scalar(Int), Scalar(Int)) if !divide => Ok(Scalar(Int)),
        (Scalar(Int | Float), Scalar(Int | Float)) => Ok(Scalar(Float)),
        _ => Err("operator requires numeric operands".into()),
    }
}

fn require_types(
    left: &EiyashouType,
    right: &EiyashouType,
    expected: &EiyashouType,
) -> Result<(), String> {
    require_type(left, expected)?;
    require_type(right, expected)
}

fn require_type(actual: &EiyashouType, expected: &EiyashouType) -> Result<(), String> {
    (actual == expected)
        .then_some(())
        .ok_or_else(|| format!("expected {expected:?}, found {actual:?}"))
}

fn types_compatible(left: &EiyashouType, right: &EiyashouType) -> bool {
    left == right
        || matches!(
            (left, right),
            (
                EiyashouType::Scalar(EiyashouScalarType::Int),
                EiyashouType::Scalar(EiyashouScalarType::Float)
            ) | (
                EiyashouType::Scalar(EiyashouScalarType::Float),
                EiyashouType::Scalar(EiyashouScalarType::Int)
            ) | (
                EiyashouType::List(EiyashouScalarType::Int),
                EiyashouType::List(EiyashouScalarType::Float)
            ) | (
                EiyashouType::List(EiyashouScalarType::Float),
                EiyashouType::List(EiyashouScalarType::Int)
            )
        )
}

fn validate_action_types(
    action: &Action,
    variables: &HashMap<String, EiyashouType>,
) -> Result<(), String> {
    use EiyashouScalarType::{Bool, Int};
    use EiyashouType::{List, Scalar};
    let validate_text = |text: &EiyashouText| {
        for part in &text.parts {
            if let EiyashouTextPart::Expression(expression) = part
                && matches!(infer_expression_type(expression, variables)?, List(_))
            {
                return Err("lists cannot be interpolated directly".into());
            }
        }
        Ok(())
    };
    match action {
        Action::EiyashouSay(dialogue) => validate_text(&dialogue.text),
        Action::EiyashouMenu { prompt, choices } => {
            validate_text(prompt)?;
            for choice in choices {
                validate_text(&choice.text)?;
                if let Some(condition) = &choice.show_when {
                    require_type(&infer_expression_type(condition, variables)?, &Scalar(Bool))?;
                }
            }
            Ok(())
        }
        Action::EiyashouSet {
            target,
            expression,
            operation,
            initialize_once,
        } => {
            let right = infer_expression_type(expression, variables)?;
            let target_type = match target {
                EiyashouPlace::Variable(name) => variables
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("unknown variable `{name}`"))?,
                EiyashouPlace::Index { variable, index } => {
                    require_type(&infer_expression_type(index, variables)?, &Scalar(Int))?;
                    match variables.get(variable) {
                        Some(List(item)) => Scalar(*item),
                        Some(_) => return Err(format!("`{variable}` is not a list")),
                        None => return Err(format!("unknown variable `{variable}`")),
                    }
                }
            };
            if !types_compatible(&target_type, &right) {
                return Err("assignment type does not match its target".into());
            }
            if *initialize_once && !matches!(target, EiyashouPlace::Variable(_)) {
                return Err("`let` cannot initialize a list element".into());
            }
            if *operation == EiyashouAssignOp::Remainder
                && (target_type != Scalar(Int) || right != Scalar(Int))
            {
                return Err("`%=` requires int operands".into());
            }
            if *operation != EiyashouAssignOp::Replace {
                let operator = match operation {
                    EiyashouAssignOp::Add => EiyashouBinaryOp::Add,
                    EiyashouAssignOp::Subtract => EiyashouBinaryOp::Subtract,
                    EiyashouAssignOp::Multiply => EiyashouBinaryOp::Multiply,
                    EiyashouAssignOp::Divide => EiyashouBinaryOp::Divide,
                    EiyashouAssignOp::Remainder => EiyashouBinaryOp::Remainder,
                    EiyashouAssignOp::Replace => unreachable!(),
                };
                infer_expression_type(
                    &EiyashouExpr::Binary {
                        op: operator,
                        left: Box::new(type_placeholder(&target_type)),
                        right: Box::new(type_placeholder(&right)),
                    },
                    &HashMap::new(),
                )?;
            }
            Ok(())
        }
        Action::EiyashouList {
            variable,
            operation,
        } => {
            let Some(List(item)) = variables.get(variable) else {
                return Err(format!("`{variable}` is not a declared list"));
            };
            let item_type = Scalar(*item);
            match operation {
                EiyashouListOperation::Append(value) | EiyashouListOperation::Remove(value) => {
                    let value = infer_expression_type(value, variables)?;
                    if types_compatible(&item_type, &value) {
                        Ok(())
                    } else {
                        Err("list mutation value has the wrong type".into())
                    }
                }
                EiyashouListOperation::Clear => Ok(()),
                EiyashouListOperation::Insert { index, value } => {
                    require_type(&infer_expression_type(index, variables)?, &Scalar(Int))?;
                    if constant_int(index).is_some_and(|value| value < 0) {
                        return Err("list indices must not be negative".into());
                    }
                    let value = infer_expression_type(value, variables)?;
                    if types_compatible(&item_type, &value) {
                        Ok(())
                    } else {
                        Err("list insertion value has the wrong type".into())
                    }
                }
                EiyashouListOperation::Pop { index, into } => {
                    if let Some(index) = index {
                        require_type(&infer_expression_type(index, variables)?, &Scalar(Int))?;
                        if constant_int(index).is_some_and(|value| value < 0) {
                            return Err("list indices must not be negative".into());
                        }
                    }
                    let target = variables
                        .get(into)
                        .ok_or_else(|| format!("unknown pop target `{into}`"))?;
                    if types_compatible(&item_type, target) {
                        Ok(())
                    } else {
                        Err("pop target type does not match list element type".into())
                    }
                }
            }
        }
        Action::EiyashouJumpIf { condition, .. } => {
            require_type(&infer_expression_type(condition, variables)?, &Scalar(Bool))
        }
        _ => Ok(()),
    }
}

fn type_placeholder(value_type: &EiyashouType) -> EiyashouExpr {
    let value = match value_type {
        EiyashouType::Scalar(EiyashouScalarType::Bool) => Value::Bool(false),
        EiyashouType::Scalar(EiyashouScalarType::Int) => Value::Int(0),
        EiyashouType::Scalar(EiyashouScalarType::Float) => Value::Float(0.0),
        EiyashouType::Scalar(EiyashouScalarType::String) => Value::Str(String::new()),
        EiyashouType::List(scalar) => return EiyashouExpr::EmptyList(*scalar),
    };
    EiyashouExpr::Literal(value)
}

fn constant_int(expression: &EiyashouExpr) -> Option<i64> {
    match expression {
        EiyashouExpr::Literal(Value::Int(value)) => Some(*value),
        EiyashouExpr::Unary {
            op: EiyashouUnaryOp::Negate,
            value,
        } => constant_int(value)?.checked_neg(),
        _ => None,
    }
}

fn definite_initialization_errors(
    actions: &[Action],
    declarations: &HashMap<String, (usize, usize, EiyashouExpr)>,
) -> Vec<(usize, String)> {
    if actions.is_empty() {
        return Vec::new();
    }
    let local_declarations = actions
        .iter()
        .filter_map(|action| match action {
            Action::EiyashouSet {
                target: EiyashouPlace::Variable(name),
                initialize_once: true,
                ..
            } => Some(name.clone()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let entry = declarations
        .keys()
        .filter(|name| !local_declarations.contains(*name))
        .cloned()
        .collect::<HashSet<_>>();
    let labels = actions
        .iter()
        .enumerate()
        .filter_map(|(index, action)| match action {
            Action::Label(label) => Some((label.clone(), index)),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let mut incoming = vec![None::<HashSet<String>>; actions.len()];
    incoming[0] = Some(entry);
    let mut queue = VecDeque::from([0usize]);
    while let Some(index) = queue.pop_front() {
        let mut after = incoming[index].clone().unwrap_or_default();
        if let Action::EiyashouSet {
            target: EiyashouPlace::Variable(name),
            initialize_once: true,
            ..
        } = &actions[index]
        {
            after.insert(name.clone());
        }
        for successor in action_successors(index, actions, &labels) {
            let changed = match &mut incoming[successor] {
                Some(existing) => {
                    let intersection = existing.intersection(&after).cloned().collect();
                    if *existing == intersection {
                        false
                    } else {
                        *existing = intersection;
                        true
                    }
                }
                slot @ None => {
                    *slot = Some(after.clone());
                    true
                }
            };
            if changed {
                queue.push_back(successor);
            }
        }
    }
    let mut errors = Vec::new();
    for index in 0..actions.len() {
        let Some(initialized) = &incoming[index] else {
            continue;
        };
        let mut reads = HashSet::new();
        collect_action_reads(&actions[index], &mut reads);
        for variable in reads.difference(initialized) {
            errors.push((
                index,
                format!("variable `{variable}` may be uninitialized on this path"),
            ));
        }
    }
    errors
}

fn action_successors(
    index: usize,
    actions: &[Action],
    labels: &HashMap<String, usize>,
) -> Vec<usize> {
    let next = (index + 1 < actions.len()).then_some(index + 1);
    match &actions[index] {
        Action::Jump(label) => labels.get(label).copied().into_iter().collect(),
        Action::EiyashouJumpIf { label, .. } => {
            labels.get(label).copied().into_iter().chain(next).collect()
        }
        Action::EiyashouMenu { choices, .. } => choices
            .iter()
            .filter_map(|choice| match &choice.target {
                ChoiceTarget::Label(label) => labels.get(label).copied(),
                _ => None,
            })
            .collect(),
        Action::ChangeScene(_) | Action::End | Action::ReturnScene => Vec::new(),
        _ => next.into_iter().collect(),
    }
}

fn collect_action_reads(action: &Action, reads: &mut HashSet<String>) {
    match action {
        Action::EiyashouSay(dialogue) => collect_text_reads(&dialogue.text, reads),
        Action::EiyashouMenu { prompt, choices } => {
            collect_text_reads(prompt, reads);
            for choice in choices {
                collect_text_reads(&choice.text, reads);
                if let Some(condition) = &choice.show_when {
                    collect_expression_reads(condition, reads);
                }
            }
        }
        Action::EiyashouSet {
            target,
            expression,
            initialize_once,
            ..
        } => {
            collect_expression_reads(expression, reads);
            match target {
                EiyashouPlace::Variable(name) if !initialize_once => {
                    reads.insert(name.clone());
                }
                EiyashouPlace::Index { variable, index } => {
                    reads.insert(variable.clone());
                    collect_expression_reads(index, reads);
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
                    collect_expression_reads(value, reads)
                }
                EiyashouListOperation::Insert { index, value } => {
                    collect_expression_reads(index, reads);
                    collect_expression_reads(value, reads);
                }
                EiyashouListOperation::Pop { index, into } => {
                    if let Some(index) = index {
                        collect_expression_reads(index, reads);
                    }
                    reads.insert(into.clone());
                }
                EiyashouListOperation::Clear => {}
            }
        }
        Action::EiyashouJumpIf { condition, .. } => collect_expression_reads(condition, reads),
        _ => {}
    }
}

fn collect_text_reads(text: &EiyashouText, reads: &mut HashSet<String>) {
    for part in &text.parts {
        if let EiyashouTextPart::Expression(expression) = part {
            collect_expression_reads(expression, reads);
        }
    }
}

fn collect_expression_reads(expression: &EiyashouExpr, reads: &mut HashSet<String>) {
    match expression {
        EiyashouExpr::Variable(name) => {
            reads.insert(name.clone());
        }
        EiyashouExpr::List(values) => {
            for value in values {
                collect_expression_reads(value, reads);
            }
        }
        EiyashouExpr::Unary { value, .. } | EiyashouExpr::Length(value) => {
            collect_expression_reads(value, reads);
        }
        EiyashouExpr::Binary { left, right, .. } => {
            collect_expression_reads(left, reads);
            collect_expression_reads(right, reads);
        }
        EiyashouExpr::Index { list, index } => {
            collect_expression_reads(list, reads);
            collect_expression_reads(index, reads);
        }
        EiyashouExpr::Literal(_) | EiyashouExpr::EmptyList(_) => {}
    }
}

fn lex(source: &str) -> (Vec<NativeToken>, Vec<Diagnostic>) {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut diagnostics = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        let start = cursor;
        let byte = bytes[cursor];
        if byte.is_ascii_whitespace() {
            cursor += 1;
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            push_token(&mut tokens, NativeTokenKind::Whitespace, start, cursor);
            continue;
        }
        if source[start..].starts_with("//") {
            cursor += 2;
            while cursor < bytes.len() && bytes[cursor] != b'\n' {
                cursor += 1;
            }
            push_token(&mut tokens, NativeTokenKind::Comment, start, cursor);
            continue;
        }
        if source[start..].starts_with("/*") {
            cursor += 2;
            let mut depth = 1usize;
            while cursor < bytes.len() && depth > 0 {
                if source[cursor..].starts_with("/*") {
                    depth += 1;
                    cursor += 2;
                } else if source[cursor..].starts_with("*/") {
                    depth -= 1;
                    cursor += 2;
                } else {
                    cursor += source[cursor..].chars().next().unwrap().len_utf8();
                }
            }
            if depth != 0 {
                diagnostics.push(error_at(source, start, "unterminated block comment"));
            }
            push_token(&mut tokens, NativeTokenKind::Comment, start, cursor);
            continue;
        }
        if byte == b'"' {
            cursor += 1;
            let mut closed = false;
            while cursor < bytes.len() {
                match bytes[cursor] {
                    b'\\' => {
                        cursor += 1;
                        if cursor < bytes.len() {
                            cursor += source[cursor..].chars().next().unwrap().len_utf8();
                        }
                    }
                    b'"' => {
                        cursor += 1;
                        closed = true;
                        break;
                    }
                    b'\n' | b'\r' => break,
                    _ => cursor += source[cursor..].chars().next().unwrap().len_utf8(),
                }
            }
            if !closed {
                diagnostics.push(error_at(source, start, "unterminated string literal"));
            }
            push_token(&mut tokens, NativeTokenKind::String, start, cursor);
            continue;
        }
        let character = source[cursor..].chars().next().unwrap();
        if character == '_' || character.is_alphabetic() {
            cursor += character.len_utf8();
            while cursor < bytes.len() {
                let candidate = source[cursor..].chars().next().unwrap();
                if candidate == '_' || candidate.is_alphanumeric() {
                    cursor += candidate.len_utf8();
                } else {
                    break;
                }
            }
            push_token(&mut tokens, NativeTokenKind::Identifier, start, cursor);
            continue;
        }
        if byte.is_ascii_digit() {
            cursor += 1;
            while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                cursor += 1;
            }
            if cursor < bytes.len() && bytes[cursor] == b'.' {
                cursor += 1;
                while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                    cursor += 1;
                }
            }
            if cursor < bytes.len() && matches!(bytes[cursor], b'e' | b'E') {
                cursor += 1;
                if cursor < bytes.len() && matches!(bytes[cursor], b'+' | b'-') {
                    cursor += 1;
                }
                while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                    cursor += 1;
                }
            }
            push_token(&mut tokens, NativeTokenKind::Number, start, cursor);
            continue;
        }
        let (kind, width) = if matches!(
            byte,
            b'{' | b'}' | b'(' | b')' | b'[' | b']' | b',' | b':' | b'@' | b'.'
        ) {
            (NativeTokenKind::Punctuation, 1)
        } else if matches!(
            byte,
            b'+' | b'-' | b'*' | b'/' | b'%' | b'=' | b'!' | b'<' | b'>'
        ) {
            let width =
                if cursor + 1 < bytes.len() && matches!(bytes[cursor + 1], b'=' | b'&' | b'|') {
                    2
                } else {
                    1
                };
            (NativeTokenKind::Operator, width)
        } else {
            diagnostics.push(error_at(
                source,
                start,
                format!("unexpected character {character:?}"),
            ));
            (NativeTokenKind::Unknown, character.len_utf8())
        };
        cursor += width;
        push_token(&mut tokens, kind, start, cursor);
    }
    (tokens, diagnostics)
}

fn push_token(tokens: &mut Vec<NativeToken>, kind: NativeTokenKind, start: usize, end: usize) {
    tokens.push(NativeToken {
        kind,
        range: start..end,
    });
}

struct Parser<'a> {
    source: &'a str,
    tokens: &'a [NativeToken],
    significant: Vec<usize>,
    cursor: usize,
    document_diagnostics: Vec<Diagnostic>,
    scene_syntax: Vec<NativeSceneSyntax>,
    current_scene: String,
    label_counter: usize,
    loop_end_labels: Vec<String>,
    source_occurrences: HashMap<String, usize>,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str, tokens: &'a [NativeToken], diagnostics: Vec<Diagnostic>) -> Self {
        let significant = tokens
            .iter()
            .enumerate()
            .filter_map(|(index, token)| {
                (!matches!(
                    token.kind,
                    NativeTokenKind::Whitespace | NativeTokenKind::Comment
                ))
                .then_some(index)
            })
            .collect();
        Self {
            source,
            tokens,
            significant,
            cursor: 0,
            document_diagnostics: diagnostics,
            scene_syntax: Vec::new(),
            current_scene: String::new(),
            label_counter: 0,
            loop_end_labels: Vec::new(),
            source_occurrences: HashMap::new(),
        }
    }

    fn parse_file(&mut self) -> Vec<ParsedScene> {
        let mut scenes = Vec::new();
        let mut names = std::collections::HashSet::new();
        while !self.eof() {
            let start = self.offset();
            if !self.eat("scene") {
                self.document_diagnostics
                    .push(self.error("expected `scene` declaration"));
                self.advance();
                continue;
            }
            let name_start = self.offset();
            let Some(name) = self.take_identifier() else {
                self.document_diagnostics
                    .push(self.error("expected scene name"));
                self.recover_top_level();
                continue;
            };
            let mut report = ParseReport::default();
            if !names.insert(name.clone()) {
                report
                    .diagnostics
                    .push(self.error(format!("duplicate scene name `{name}`")));
            }
            self.current_scene.clone_from(&name);
            self.source_occurrences.clear();
            if !self.eat("{") {
                report
                    .diagnostics
                    .push(self.error("expected `{` after scene name"));
                self.recover_top_level();
            } else {
                self.parse_block(&mut report);
            }
            let end = self.previous_end().max(start);
            self.scene_syntax.push(NativeSceneSyntax {
                name: name.clone(),
                name_range: name_start..name_start + name.len(),
                range: start..end,
            });
            scenes.push(ParsedScene {
                name: Some(name),
                report,
            });
        }

        if scenes.is_empty() {
            let mut report = ParseReport::default();
            report
                .diagnostics
                .extend(self.document_diagnostics.iter().cloned());
            if report.diagnostics.is_empty() {
                report.diagnostics.push(error_at(
                    self.source,
                    0,
                    "native source must declare at least one scene",
                ));
            }
            return vec![ParsedScene { name: None, report }];
        }
        if !self.document_diagnostics.is_empty() {
            scenes[0]
                .report
                .diagnostics
                .extend(self.document_diagnostics.iter().cloned());
        }
        scenes
    }

    fn parse_block(&mut self, report: &mut ParseReport) {
        if self.eat("}") {
            return;
        }
        loop {
            let start = self.offset();
            let actions = self.parse_statement(report);
            let span = span_at(self.source, start);
            for action in actions {
                report.push(action, span);
            }
            if self.eat("}") {
                break;
            }
            if self.eof() {
                report
                    .diagnostics
                    .push(error_at(self.source, start, "unterminated scene block"));
                break;
            }
            if !self.eat(",") {
                report
                    .diagnostics
                    .push(self.error("expected `,` between statements"));
                self.recover_statement();
                if self.eat("}") {
                    break;
                }
            } else if self.peek_text() == Some("}") {
                report
                    .diagnostics
                    .push(self.error("trailing commas are not allowed"));
                self.eat("}");
                break;
            }
        }
    }

    fn parse_statement(&mut self, report: &mut ParseReport) -> Vec<Action> {
        let explicit_id = self.take_annotation(report);
        if self.peek_kind() == Some(NativeTokenKind::String) {
            return self.parse_narration(explicit_id, report);
        }
        let Some(name) = self.take_identifier() else {
            report.diagnostics.push(self.error("expected statement"));
            self.advance();
            return Vec::new();
        };
        match name.as_str() {
            "choice" => return self.parse_choice(explicit_id, report),
            "if" => return self.parse_if(report),
            "loop" => return self.parse_loop(report),
            "let" => return self.parse_let(report),
            "break" => return self.parse_break(report),
            "return" => {
                self.reject_annotation(explicit_id, report);
                return vec![Action::ReturnScene];
            }
            _ => {}
        }
        if self.eat(":") {
            return self.parse_dialogue(name, explicit_id, report);
        }
        self.reject_annotation(explicit_id, report);
        if self.peek_text() == Some(".")
            || matches!(
                self.peek_text(),
                Some("=" | "+=" | "-=" | "*=" | "/=" | "%=" | "[")
            )
        {
            return self.parse_assignment_or_list(name, report);
        }
        if self.peek_text() == Some("(") {
            return self.parse_command(name, report).into_iter().collect();
        }
        report
            .diagnostics
            .push(self.error("expected assignment, list mutation, dialogue, or command"));
        self.skip_statement_shape();
        Vec::new()
    }

    fn parse_narration(
        &mut self,
        explicit_id: Option<String>,
        report: &mut ParseReport,
    ) -> Vec<Action> {
        let Some(text) = self.take_eiyashou_text(report) else {
            return Vec::new();
        };
        let vocal = self.take_optional_voice();
        let source_id = self.source_id(explicit_id, "", &text);
        vec![Action::EiyashouSay(EiyashouDialogue {
            speaker: String::new(),
            speaker_color: None,
            text,
            options: SayOptions {
                vocal,
                ..SayOptions::default()
            },
            source_id,
        })]
    }

    fn parse_dialogue(
        &mut self,
        speaker: String,
        explicit_id: Option<String>,
        report: &mut ParseReport,
    ) -> Vec<Action> {
        if self.eat("{") {
            let mut actions = Vec::new();
            if self.eat("}") {
                report
                    .diagnostics
                    .push(self.error("dialogue blocks must not be empty"));
                return actions;
            }
            loop {
                let entry_id = self.take_annotation(report);
                let Some(text) = self.take_eiyashou_text(report) else {
                    self.recover_statement();
                    break;
                };
                let vocal = self.take_optional_voice();
                let source_id = self.source_id(entry_id, &speaker, &text);
                actions.push(Action::EiyashouSay(EiyashouDialogue {
                    speaker: speaker.clone(),
                    speaker_color: None,
                    text,
                    options: SayOptions {
                        vocal,
                        ..SayOptions::default()
                    },
                    source_id,
                }));
                if self.eat("}") {
                    break;
                }
                if !self.eat(",") {
                    report
                        .diagnostics
                        .push(self.error("expected `,` in dialogue block"));
                    self.recover_statement();
                    break;
                }
                if self.peek_text() == Some("}") {
                    report
                        .diagnostics
                        .push(self.error("trailing commas are not allowed"));
                    self.eat("}");
                    break;
                }
            }
            self.reject_annotation(explicit_id, report);
            return actions;
        }
        let Some(text) = self.take_eiyashou_text(report) else {
            report
                .diagnostics
                .push(self.error("expected string or dialogue block after `:`"));
            return Vec::new();
        };
        let vocal = self.take_optional_voice();
        let source_id = self.source_id(explicit_id, &speaker, &text);
        vec![Action::EiyashouSay(EiyashouDialogue {
            speaker,
            speaker_color: None,
            text,
            options: SayOptions {
                vocal,
                ..SayOptions::default()
            },
            source_id,
        })]
    }

    fn parse_choice(
        &mut self,
        explicit_id: Option<String>,
        report: &mut ParseReport,
    ) -> Vec<Action> {
        self.reject_annotation(explicit_id, report);
        let prompt = if self.eat("(") {
            let prompt = self
                .take_eiyashou_text(report)
                .unwrap_or_else(|| EiyashouText::literal(""));
            if !self.eat(")") {
                report
                    .diagnostics
                    .push(self.error("expected `)` after choice prompt"));
            }
            prompt
        } else {
            EiyashouText::literal("")
        };
        if !self.eat("{") {
            report
                .diagnostics
                .push(self.error("expected `{` after choice"));
            return Vec::new();
        }
        let mut choices = Vec::new();
        let mut branches = Vec::<(String, Vec<Action>)>::new();
        let merge_label = self.next_label("choice_merge");
        while !self.eof() && self.peek_text() != Some("}") {
            let option_id = self.take_annotation(report);
            let Some(text) = self.take_eiyashou_text(report) else {
                report
                    .diagnostics
                    .push(self.error("expected choice option text"));
                self.recover_statement();
                break;
            };
            let show_when = self
                .eat("when")
                .then(|| self.take_parenthesized_expression(report))
                .flatten();
            if !self.eat(":") {
                report
                    .diagnostics
                    .push(self.error("expected `:` after choice option"));
                self.recover_statement();
                break;
            }
            let label = self.next_label("choice_branch");
            let actions = if self.eat("{") {
                self.parse_nested_block(report)
            } else {
                self.parse_statement(report)
            };
            let target = (!actions.is_empty()).then(|| ChoiceTarget::Label(label.clone()));
            if !actions.is_empty() {
                branches.push((label, actions));
            }
            if let Some(target) = target {
                let source_id = self.source_id(option_id, "choice", &text);
                choices.push(EiyashouChoice {
                    text,
                    target,
                    show_when,
                    source_id,
                });
            }
            if self.peek_text() == Some("}") {
                break;
            }
            if !self.eat(",") {
                report
                    .diagnostics
                    .push(self.error("expected `,` between choice options"));
                self.recover_statement();
                break;
            }
            if self.peek_text() == Some("}") {
                report
                    .diagnostics
                    .push(self.error("trailing commas are not allowed"));
                break;
            }
        }
        self.eat("}");
        if choices.is_empty() {
            report
                .diagnostics
                .push(self.error("choice must contain a valid option"));
            Vec::new()
        } else {
            if choices.len() == 1 {
                report.diagnostics.push(Diagnostic {
                    level: DiagnosticLevel::Warning,
                    span: self.span(),
                    message: "choice has only one option".into(),
                });
            }
            let mut actions = vec![Action::EiyashouMenu { prompt, choices }];
            for (label, mut branch) in branches {
                actions.push(Action::Label(label));
                actions.append(&mut branch);
                actions.push(Action::Jump(merge_label.clone()));
            }
            if actions.len() > 1 {
                actions.push(Action::Label(merge_label));
            }
            actions
        }
    }

    fn take_annotation(&mut self, report: &mut ParseReport) -> Option<String> {
        if !self.eat("@") {
            return None;
        }
        match self.take_identifier() {
            Some(identifier) => Some(identifier),
            None => {
                report
                    .diagnostics
                    .push(self.error("expected identifier after `@`"));
                None
            }
        }
    }

    fn reject_annotation(&self, annotation: Option<String>, report: &mut ParseReport) {
        if annotation.is_some() {
            report.diagnostics.push(
                self.error("stable `@id` is only valid on dialogue, narration, and choice options"),
            );
        }
    }

    fn source_id(
        &mut self,
        explicit: Option<String>,
        speaker: &str,
        text: &EiyashouText,
    ) -> String {
        if let Some(explicit) = explicit {
            return explicit;
        }
        let semantic = format!("{}\0{}\0{text:?}", self.current_scene, speaker);
        let occurrence = self.source_occurrences.entry(semantic.clone()).or_default();
        *occurrence += 1;
        let mut hash = 0xcbf29ce484222325u64;
        for byte in semantic.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        format!("{}:{hash:016x}:{}", self.current_scene, *occurrence)
    }

    fn next_label(&mut self, purpose: &str) -> String {
        let label = format!(
            "__eiyashou_{}_{}_{}",
            self.current_scene, purpose, self.label_counter
        );
        self.label_counter += 1;
        label
    }

    fn parse_nested_block(&mut self, report: &mut ParseReport) -> Vec<Action> {
        let mut actions = Vec::new();
        if self.eat("}") {
            return actions;
        }
        loop {
            actions.extend(self.parse_statement(report));
            if self.eat("}") {
                break;
            }
            if self.eof() {
                report
                    .diagnostics
                    .push(self.error("unterminated statement block"));
                break;
            }
            if !self.eat(",") {
                report
                    .diagnostics
                    .push(self.error("expected `,` between statements"));
                self.recover_statement();
                if self.eat("}") {
                    break;
                }
            } else if self.peek_text() == Some("}") {
                report
                    .diagnostics
                    .push(self.error("trailing commas are not allowed"));
                self.eat("}");
                break;
            }
        }
        actions
    }

    fn take_eiyashou_text(&mut self, report: &mut ParseReport) -> Option<EiyashouText> {
        if self.peek_kind() != Some(NativeTokenKind::String) {
            return None;
        }
        let index = self.current_token_index()?;
        self.advance();
        let raw = self.text(index);
        let offset = self.tokens[index].range.start;
        Some(parse_eiyashou_text(
            self.source,
            raw,
            offset,
            &mut report.diagnostics,
        ))
    }

    fn expression_from_indices(
        &self,
        indices: &[usize],
        report: &mut ParseReport,
    ) -> Option<EiyashouExpr> {
        if indices.is_empty() {
            report.diagnostics.push(self.error("expected expression"));
            return None;
        }
        let mut parser = ExpressionParser::new(self.source, self.tokens, indices);
        match parser.parse() {
            Ok(expression) => Some(expression),
            Err(message) => {
                let offset = indices
                    .get(parser.cursor.min(indices.len().saturating_sub(1)))
                    .map_or(self.offset(), |index| self.tokens[*index].range.start);
                report
                    .diagnostics
                    .push(error_at(self.source, offset, message));
                None
            }
        }
    }

    fn take_parenthesized_expression(&mut self, report: &mut ParseReport) -> Option<EiyashouExpr> {
        if !self.eat("(") {
            report
                .diagnostics
                .push(self.error("expected `(` before expression"));
            return None;
        }
        let start = self.cursor;
        let mut depth = 0usize;
        while !self.eof() {
            match self.peek_text() {
                Some("(") => {
                    depth += 1;
                    self.advance();
                }
                Some(")") if depth == 0 => break,
                Some(")") => {
                    depth -= 1;
                    self.advance();
                }
                _ => self.advance(),
            }
        }
        let indices = self.significant[start..self.cursor].to_vec();
        if !self.eat(")") {
            report
                .diagnostics
                .push(self.error("expected `)` after expression"));
        }
        self.expression_from_indices(&indices, report)
    }

    fn take_statement_expression(&mut self, report: &mut ParseReport) -> Option<EiyashouExpr> {
        let start = self.cursor;
        let mut depth = 0usize;
        while !self.eof() {
            match self.peek_text() {
                Some("(" | "[") => {
                    depth += 1;
                    self.advance();
                }
                Some(")" | "]") if depth > 0 => {
                    depth -= 1;
                    self.advance();
                }
                Some("," | "}") if depth == 0 => break,
                _ => self.advance(),
            }
        }
        let indices = self.significant[start..self.cursor].to_vec();
        self.expression_from_indices(&indices, report)
    }

    fn parse_let(&mut self, report: &mut ParseReport) -> Vec<Action> {
        let Some(name) = self.take_identifier() else {
            report
                .diagnostics
                .push(self.error("expected variable name after `let`"));
            return Vec::new();
        };
        if !self.eat("=") {
            report
                .diagnostics
                .push(self.error("expected `=` in variable declaration"));
            return Vec::new();
        }
        self.take_statement_expression(report)
            .map(|expression| {
                vec![Action::EiyashouSet {
                    target: EiyashouPlace::Variable(name),
                    expression,
                    operation: EiyashouAssignOp::Replace,
                    initialize_once: true,
                }]
            })
            .unwrap_or_default()
    }

    fn parse_assignment_or_list(
        &mut self,
        variable: String,
        report: &mut ParseReport,
    ) -> Vec<Action> {
        if self.eat(".") {
            let Some(method) = self.take_identifier() else {
                report
                    .diagnostics
                    .push(self.error("expected list method after `.`"));
                return Vec::new();
            };
            let args = self.take_call_args(report);
            let expression = |argument: &Argument, report: &mut ParseReport| {
                self.expression_from_indices(&argument.token_indices, report)
            };
            let operation = match method.as_str() {
                "append" | "remove" => {
                    self.validate_signature(&method, &args, 1, &[], report);
                    args.first()
                        .and_then(|argument| expression(argument, report))
                        .map(|value| {
                            if method == "append" {
                                EiyashouListOperation::Append(value)
                            } else {
                                EiyashouListOperation::Remove(value)
                            }
                        })
                }
                "clear" => {
                    self.validate_signature(&method, &args, 0, &[], report);
                    Some(EiyashouListOperation::Clear)
                }
                "insert" => {
                    self.validate_signature(&method, &args, 2, &[], report);
                    args.first()
                        .and_then(|argument| expression(argument, report))
                        .zip(
                            args.get(1)
                                .and_then(|argument| expression(argument, report)),
                        )
                        .map(|(index, value)| EiyashouListOperation::Insert { index, value })
                }
                _ => {
                    report
                        .diagnostics
                        .push(self.error(format!("unknown list method `{method}`")));
                    None
                }
            };
            return operation
                .map(|operation| {
                    vec![Action::EiyashouList {
                        variable,
                        operation,
                    }]
                })
                .unwrap_or_default();
        }

        let target = if self.eat("[") {
            let start = self.cursor;
            let mut depth = 0usize;
            while !self.eof() {
                match self.peek_text() {
                    Some("[") => {
                        depth += 1;
                        self.advance();
                    }
                    Some("]") if depth == 0 => break,
                    Some("]") => {
                        depth -= 1;
                        self.advance();
                    }
                    _ => self.advance(),
                }
            }
            let indices = self.significant[start..self.cursor].to_vec();
            let index = self.expression_from_indices(&indices, report);
            if !self.eat("]") {
                report
                    .diagnostics
                    .push(self.error("expected `]` after list index"));
            }
            index.map(|index| EiyashouPlace::Index {
                variable: variable.clone(),
                index,
            })
        } else {
            Some(EiyashouPlace::Variable(variable))
        };
        let operator = match self.peek_text() {
            Some("=") => EiyashouAssignOp::Replace,
            Some("+=") => EiyashouAssignOp::Add,
            Some("-=") => EiyashouAssignOp::Subtract,
            Some("*=") => EiyashouAssignOp::Multiply,
            Some("/=") => EiyashouAssignOp::Divide,
            Some("%=") => EiyashouAssignOp::Remainder,
            _ => {
                report
                    .diagnostics
                    .push(self.error("expected assignment operator"));
                return Vec::new();
            }
        };
        self.advance();
        target
            .zip(self.take_statement_expression(report))
            .map(|(target, expression)| {
                vec![Action::EiyashouSet {
                    target,
                    expression,
                    operation: operator,
                    initialize_once: false,
                }]
            })
            .unwrap_or_default()
    }

    fn parse_if(&mut self, report: &mut ParseReport) -> Vec<Action> {
        let mut clauses = Vec::new();
        let Some(condition) = self.take_parenthesized_expression(report) else {
            return Vec::new();
        };
        if !self.eat("{") {
            report
                .diagnostics
                .push(self.error("expected `{` after if condition"));
            return Vec::new();
        }
        clauses.push((condition, self.parse_nested_block(report)));
        while self.eat("else") {
            if self.eat("if") {
                let Some(condition) = self.take_parenthesized_expression(report) else {
                    return Vec::new();
                };
                if !self.eat("{") {
                    report
                        .diagnostics
                        .push(self.error("expected `{` after else-if condition"));
                    return Vec::new();
                }
                clauses.push((condition, self.parse_nested_block(report)));
            } else {
                if !self.eat("{") {
                    report
                        .diagnostics
                        .push(self.error("expected `{` after else"));
                    return Vec::new();
                }
                let fallback = self.parse_nested_block(report);
                return self.lower_if(clauses, fallback);
            }
        }
        self.lower_if(clauses, Vec::new())
    }

    fn lower_if(
        &mut self,
        clauses: Vec<(EiyashouExpr, Vec<Action>)>,
        fallback: Vec<Action>,
    ) -> Vec<Action> {
        let end = self.next_label("if_end");
        let mut actions = Vec::new();
        for (condition, mut body) in clauses {
            let next = self.next_label("if_next");
            actions.push(Action::EiyashouJumpIf {
                condition,
                label: next.clone(),
                jump_when: false,
            });
            actions.append(&mut body);
            actions.push(Action::Jump(end.clone()));
            actions.push(Action::Label(next));
        }
        actions.extend(fallback);
        actions.push(Action::Label(end));
        actions
    }

    fn parse_loop(&mut self, report: &mut ParseReport) -> Vec<Action> {
        if !self.eat("{") {
            report
                .diagnostics
                .push(self.error("expected `{` after loop"));
            return Vec::new();
        }
        let start = self.next_label("loop_start");
        let end = self.next_label("loop_end");
        self.loop_end_labels.push(end.clone());
        let mut body = self.parse_nested_block(report);
        self.loop_end_labels.pop();
        let mut actions = vec![Action::Label(start.clone())];
        actions.append(&mut body);
        actions.push(Action::Jump(start));
        actions.push(Action::Label(end));
        actions
    }

    fn parse_break(&mut self, report: &mut ParseReport) -> Vec<Action> {
        match self.loop_end_labels.last() {
            Some(label) => vec![Action::Jump(label.clone())],
            None => {
                report
                    .diagnostics
                    .push(self.error("`break` is only valid inside a loop"));
                Vec::new()
            }
        }
    }

    fn parse_command(&mut self, name: String, report: &mut ParseReport) -> Option<Action> {
        let args = self.take_call_args(report);
        match name.as_str() {
            "goto" | "call" | "wait" => {
                self.validate_signature(&name, &args, 1, &[], report);
            }
            "background" => {
                self.validate_signature(&name, &args, 1, &["transition"], report);
            }
            "sprite" => {
                self.validate_signature(&name, &args, 2, &["position", "transition", "z"], report);
            }
            "hide" => {
                self.validate_signature(&name, &args, 1, &["transition"], report);
            }
            "move" => {
                self.validate_signature(&name, &args, 2, &["duration", "easing"], report);
            }
            "bgm" => {
                self.validate_signature(&name, &args, 1, &["volume", "fade", "loop"], report);
            }
            "se" => {
                self.validate_signature(&name, &args, 1, &["volume"], report);
            }
            "video" => {
                self.validate_signature(&name, &args, 1, &["skippable"], report);
            }
            "pop" => {
                let positional = args
                    .iter()
                    .filter(|argument| argument.name.is_none())
                    .count();
                if !(1..=2).contains(&positional) {
                    report
                        .diagnostics
                        .push(self.error("`pop` requires a list and optional index"));
                }
                for name in args.iter().filter_map(|argument| argument.name.as_deref()) {
                    if name != "into" {
                        report
                            .diagnostics
                            .push(self.error(format!("unknown named argument `{name}` for `pop`")));
                    }
                }
            }
            _ => {}
        }
        let first = args.first();
        match name.as_str() {
            "goto" | "call" => {
                let scene = first.and_then(|arg| self.argument_identifier(arg));
                match (name.as_str(), scene) {
                    ("goto", Some(scene)) => Some(Action::ChangeScene(scene)),
                    ("call", Some(scene)) => Some(Action::CallScene(scene)),
                    _ => {
                        report
                            .diagnostics
                            .push(self.error(format!("{name}(...) requires one scene identifier")));
                        None
                    }
                }
            }
            "wait" => match first.and_then(|arg| self.argument_duration(arg)) {
                Some(seconds) => Some(Action::Wait { seconds }),
                None => {
                    report
                        .diagnostics
                        .push(self.error("wait(...) requires an `ms` or `s` duration"));
                    None
                }
            },
            "background" => {
                let image = first.and_then(|arg| self.argument_identifier(arg));
                let transition = self.named_transition(&args, report);
                match image.as_deref() {
                    Some("none") => Some(Action::HideBg { transition }),
                    Some(image) => Some(Action::ShowBg {
                        image: image.into(),
                        transition,
                        transform: SpriteTransform::default(),
                    }),
                    None => {
                        report.diagnostics.push(
                            self.error("background(...) requires a resource identifier or `none`"),
                        );
                        None
                    }
                }
            }
            "sprite" => {
                let slot = first.and_then(|arg| self.argument_identifier(arg));
                let image = args.get(1).and_then(|arg| self.argument_identifier(arg));
                let position = self
                    .named_identifier(&args, "position")
                    .unwrap_or_else(|| "center".into());
                let position = match position.as_str() {
                    "left" => Position::left(0.0),
                    "center" => Position::center(0.0),
                    "right" => Position::right(0.0),
                    _ => {
                        report.diagnostics.push(
                            self.error("sprite position must be `left`, `center`, or `right`"),
                        );
                        Position::center(0.0)
                    }
                };
                let z_index = self.named_number(&args, "z").unwrap_or(0.0) as i32;
                match (slot, image) {
                    (Some(id), Some(image)) => Some(Action::ShowSprite {
                        id,
                        image,
                        position,
                        layout: SpriteLayout::Natural,
                        transition: self.named_transition(&args, report),
                        transform: SpriteTransform::default(),
                        z_index,
                        blend: BlendMode::Alpha,
                    }),
                    _ => {
                        report
                            .diagnostics
                            .push(self.error("sprite(...) requires slot and image identifiers"));
                        None
                    }
                }
            }
            "hide" => {
                let target = first.and_then(|arg| self.argument_text(arg));
                let transition = self.named_transition(&args, report);
                match target.as_deref() {
                    Some("*") => Some(Action::HideSprites {
                        prefix: String::new(),
                        transition,
                    }),
                    Some(id) => Some(Action::HideSprite {
                        id: id.into(),
                        transition,
                    }),
                    None => {
                        report
                            .diagnostics
                            .push(self.error("hide(...) requires a sprite slot or `*`"));
                        None
                    }
                }
            }
            "move" => {
                let id = first.and_then(|argument| self.argument_identifier(argument));
                let position = args
                    .get(1)
                    .and_then(|argument| self.argument_identifier(argument));
                let position = match position.as_deref() {
                    Some("left") => Some(Position::left(0.0)),
                    Some("center") => Some(Position::center(0.0)),
                    Some("right") => Some(Position::right(0.0)),
                    Some(_) => {
                        report
                            .diagnostics
                            .push(self.error("move position must be `left`, `center`, or `right`"));
                        None
                    }
                    None => None,
                };
                let duration = self.named_duration_checked(&args, "duration", report)?;
                let easing = self.named_easing(&args, "easing", report)?;
                match (id, position) {
                    (Some(id), Some(position)) => Some(Action::MoveSprite {
                        id,
                        position,
                        duration,
                        easing,
                        blocking: true,
                    }),
                    _ => {
                        report.diagnostics.push(
                            self.error("move(...) requires a sprite slot and named position"),
                        );
                        None
                    }
                }
            }
            "bgm" => {
                let file = first.and_then(|arg| self.argument_identifier(arg));
                let volume = self.named_number(&args, "volume").unwrap_or(1.0);
                if !(0.0..=1.0).contains(&volume) {
                    report
                        .diagnostics
                        .push(self.error("bgm volume must be between 0 and 1"));
                    return None;
                }
                file.map(|file| Action::EiyashouBgm {
                    file: (file != "none").then_some(file),
                    volume: volume as f32,
                    fade_seconds: self.named_duration(&args, "fade").unwrap_or(0.0),
                    looped: self.named_bool(&args, "loop").unwrap_or(true),
                })
            }
            "se" => {
                let file = first.and_then(|arg| self.argument_identifier(arg));
                file.map(|file| Action::Effect {
                    file: (file != "none").then_some(file),
                    volume: self.named_number(&args, "volume").unwrap_or(1.0) as f32,
                    id: None,
                })
            }
            "video" => {
                if self.named_arg(&args, "loop").is_some()
                    || self.named_arg(&args, "wait").is_some()
                {
                    report.diagnostics.push(self.error(
                        "video v1 has no `loop` or `wait` parameter; playback is always non-looping and blocking",
                    ));
                    return None;
                }
                let file = first.and_then(|arg| self.argument_identifier(arg));
                file.map(|file| Action::PlayVideo {
                    video: VideoSpec {
                        id: file.clone(),
                        file,
                        looped: false,
                        muted: false,
                        alpha: 1.0,
                        skippable: self.named_bool(&args, "skippable").unwrap_or(true),
                        wait_for_finished: true,
                        mode: VideoMode::Fullscreen,
                    },
                })
            }
            "pop" => {
                let Some(variable) = first.and_then(|arg| self.argument_identifier(arg)) else {
                    report
                        .diagnostics
                        .push(self.error("pop(...) requires a list variable"));
                    return None;
                };
                let Some(into) = self.named_identifier(&args, "into") else {
                    report
                        .diagnostics
                        .push(self.error("pop(...) requires `into: target`"));
                    return None;
                };
                let index = args
                    .iter()
                    .filter(|argument| argument.name.is_none())
                    .nth(1)
                    .and_then(|argument| {
                        self.expression_from_indices(&argument.token_indices, report)
                    });
                Some(Action::EiyashouList {
                    variable,
                    operation: EiyashouListOperation::Pop { index, into },
                })
            }
            "setting" => {
                report
                    .diagnostics
                    .push(self.error("generic `setting(...)` is not part of keine native DSL v1"));
                None
            }
            _ => {
                report
                    .diagnostics
                    .push(self.error(format!("unknown native command `{name}`")));
                None
            }
        }
    }

    fn take_call_args(&mut self, report: &mut ParseReport) -> Vec<Argument> {
        if !self.eat("(") {
            report.diagnostics.push(self.error("expected `(`"));
            return Vec::new();
        }
        let mut args = Vec::new();
        if self.eat(")") {
            return args;
        }
        loop {
            let start = self.cursor;
            let mut depth = 0usize;
            while !self.eof() {
                match self.peek_text() {
                    Some("(" | "[") => {
                        depth += 1;
                        self.advance();
                    }
                    Some(")") if depth == 0 => break,
                    Some(",") if depth == 0 => break,
                    Some(")" | "]") => {
                        depth = depth.saturating_sub(1);
                        self.advance();
                    }
                    _ => self.advance(),
                }
            }
            let mut token_indices = self.significant[start..self.cursor].to_vec();
            let name = if token_indices.len() >= 3
                && self.text(token_indices[0]) != "*"
                && self.text(token_indices[1]) == ":"
            {
                let name = self.text(token_indices.remove(0)).to_owned();
                token_indices.remove(0);
                Some(name)
            } else {
                None
            };
            if token_indices.is_empty() {
                report
                    .diagnostics
                    .push(self.error("argument must not be empty"));
            }
            args.push(Argument {
                name,
                token_indices,
            });
            if self.eat(")") {
                break;
            }
            if !self.eat(",") {
                report
                    .diagnostics
                    .push(self.error("expected `,` or `)` in argument list"));
                break;
            }
            if self.peek_text() == Some(")") {
                report
                    .diagnostics
                    .push(self.error("trailing commas are not allowed"));
                self.eat(")");
                break;
            }
        }
        let mut named = false;
        let mut seen = std::collections::HashSet::new();
        for arg in &args {
            if let Some(name) = &arg.name {
                named = true;
                if !seen.insert(name.clone()) {
                    report
                        .diagnostics
                        .push(self.error(format!("duplicate named argument `{name}`")));
                }
            } else if named {
                report
                    .diagnostics
                    .push(self.error("positional arguments must precede named arguments"));
            }
        }
        args
    }

    fn take_optional_voice(&mut self) -> Option<String> {
        if self.peek_text() != Some(",") {
            return None;
        }
        let saved = self.cursor;
        self.advance();
        let voice = self.take_identifier();
        if voice.is_some() && matches!(self.peek_text(), Some("," | "}") | None) {
            voice
        } else {
            self.cursor = saved;
            None
        }
    }

    fn validate_signature(
        &self,
        command: &str,
        args: &[Argument],
        positional: usize,
        named: &[&str],
        report: &mut ParseReport,
    ) {
        let actual_positional = args
            .iter()
            .filter(|argument| argument.name.is_none())
            .count();
        if actual_positional != positional {
            report.diagnostics.push(self.error(format!(
                "`{command}` requires {positional} positional argument(s), found {actual_positional}"
            )));
        }
        for name in args.iter().filter_map(|argument| argument.name.as_deref()) {
            if !named.contains(&name) {
                report
                    .diagnostics
                    .push(self.error(format!("unknown named argument `{name}` for `{command}`")));
            }
        }
    }

    fn argument_identifier(&self, argument: &Argument) -> Option<String> {
        (argument.token_indices.len() == 1
            && self.tokens[argument.token_indices[0]].kind == NativeTokenKind::Identifier)
            .then(|| self.text(argument.token_indices[0]).to_owned())
    }

    fn argument_text(&self, argument: &Argument) -> Option<String> {
        (argument.token_indices.len() == 1).then(|| self.text(argument.token_indices[0]).to_owned())
    }

    fn argument_number(&self, argument: &Argument) -> Option<f64> {
        (argument.token_indices.len() == 1)
            .then(|| self.text(argument.token_indices[0]).parse().ok())
            .flatten()
    }

    fn argument_bool(&self, argument: &Argument) -> Option<bool> {
        match self.argument_identifier(argument)?.as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }

    fn argument_duration(&self, argument: &Argument) -> Option<f32> {
        if argument.token_indices.len() != 2 {
            return None;
        }
        let value = self.text(argument.token_indices[0]).parse::<f32>().ok()?;
        if !value.is_finite() || value < 0.0 {
            return None;
        }
        match self.text(argument.token_indices[1]) {
            "ms" => Some(value / 1000.0),
            "s" => Some(value),
            _ => None,
        }
    }

    fn named_arg<'b>(&self, args: &'b [Argument], name: &str) -> Option<&'b Argument> {
        args.iter()
            .find(|argument| argument.name.as_deref() == Some(name))
    }

    fn named_identifier(&self, args: &[Argument], name: &str) -> Option<String> {
        self.named_arg(args, name)
            .and_then(|argument| self.argument_identifier(argument))
    }

    fn named_number(&self, args: &[Argument], name: &str) -> Option<f64> {
        self.named_arg(args, name)
            .and_then(|argument| self.argument_number(argument))
    }

    fn named_bool(&self, args: &[Argument], name: &str) -> Option<bool> {
        self.named_arg(args, name)
            .and_then(|argument| self.argument_bool(argument))
    }

    fn named_duration(&self, args: &[Argument], name: &str) -> Option<f32> {
        self.named_arg(args, name)
            .and_then(|argument| self.argument_duration(argument))
    }

    fn named_duration_checked(
        &self,
        args: &[Argument],
        name: &str,
        report: &mut ParseReport,
    ) -> Option<f32> {
        let Some(argument) = self.named_arg(args, name) else {
            return Some(0.0);
        };
        match self.argument_duration(argument) {
            Some(duration) => Some(duration),
            None => {
                report.diagnostics.push(self.error(format!(
                    "`{name}` requires a non-negative `ms` or `s` duration"
                )));
                None
            }
        }
    }

    fn named_easing(
        &self,
        args: &[Argument],
        name: &str,
        report: &mut ParseReport,
    ) -> Option<Easing> {
        let Some(argument) = self.named_arg(args, name) else {
            return Some(Easing::Linear);
        };
        let easing = match self.argument_identifier(argument).as_deref() {
            Some("linear") => Easing::Linear,
            Some("ease_in") => Easing::EaseIn,
            Some("ease_out") => Easing::EaseOut,
            Some("ease_in_out") => Easing::EaseInOut,
            Some("in_out_quad") => Easing::InOutQuad,
            Some("out_cubic") => Easing::OutCubic,
            Some("in_out_cubic") => Easing::InOutCubic,
            Some("out_back") => Easing::OutBack,
            Some("out_bounce") => Easing::OutBounce,
            _ => {
                report
                    .diagnostics
                    .push(self.error(format!("unknown `{name}` value")));
                return None;
            }
        };
        Some(easing)
    }

    fn named_transition(&self, args: &[Argument], report: &mut ParseReport) -> Transition {
        let Some(argument) = self.named_arg(args, "transition") else {
            return Transition::Instant;
        };
        let tokens = &argument.token_indices;
        if tokens.len() == 1 && self.text(tokens[0]) == "instant" {
            return Transition::Instant;
        }
        if tokens.len() < 4
            || self.text(tokens[1]) != "("
            || self.text(*tokens.last().unwrap()) != ")"
        {
            report.diagnostics.push(self.error("malformed transition"));
            return Transition::Instant;
        }
        let raw = tokens
            .iter()
            .map(|index| self.text(*index))
            .collect::<String>();
        let Some((transition_name, duration)) = parse_transition_text(&raw) else {
            report
                .diagnostics
                .push(self.error("transition requires an `ms` or `s` duration"));
            return Transition::Instant;
        };
        match transition_name {
            "fade" => Transition::Fade(duration),
            "slide_from_left" => Transition::SlideFromLeft(duration),
            "slide_from_right" => Transition::SlideFromRight(duration),
            "crossfade" => Transition::Crossfade(duration),
            "wipe" => Transition::Wipe(duration),
            "dissolve" => Transition::Dissolve(duration),
            _ => {
                report.diagnostics.push(self.error("unknown transition"));
                Transition::Instant
            }
        }
    }

    fn skip_statement_shape(&mut self) {
        let mut depth = 0usize;
        while !self.eof() {
            match self.peek_text() {
                Some("{" | "(" | "[") => {
                    depth += 1;
                    self.advance();
                }
                Some("}" | ")" | "]") if depth > 0 => {
                    depth -= 1;
                    self.advance();
                }
                Some("," | "}") if depth == 0 => break,
                _ => self.advance(),
            }
        }
    }

    fn recover_statement(&mut self) {
        self.skip_statement_shape();
        if self.peek_text() == Some(",") {
            self.advance();
        }
    }

    fn recover_top_level(&mut self) {
        while !self.eof() && self.peek_text() != Some("scene") {
            self.advance();
        }
    }

    fn take_identifier(&mut self) -> Option<String> {
        if self.peek_kind() != Some(NativeTokenKind::Identifier) {
            return None;
        }
        let value = self.peek_text()?.to_owned();
        self.advance();
        Some(value)
    }

    fn eat(&mut self, expected: &str) -> bool {
        if self.peek_text() == Some(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn advance(&mut self) {
        self.cursor = (self.cursor + 1).min(self.significant.len());
    }

    fn eof(&self) -> bool {
        self.cursor >= self.significant.len()
    }

    fn current_token_index(&self) -> Option<usize> {
        self.significant.get(self.cursor).copied()
    }

    fn peek_text(&self) -> Option<&'a str> {
        self.current_token_index().map(|index| self.text(index))
    }

    fn peek_kind(&self) -> Option<NativeTokenKind> {
        self.current_token_index()
            .map(|index| self.tokens[index].kind)
    }

    fn text(&self, index: usize) -> &'a str {
        &self.source[self.tokens[index].range.clone()]
    }

    fn offset(&self) -> usize {
        self.current_token_index()
            .map(|index| self.tokens[index].range.start)
            .unwrap_or(self.source.len())
    }

    fn previous_end(&self) -> usize {
        self.cursor
            .checked_sub(1)
            .and_then(|cursor| self.significant.get(cursor))
            .map(|index| self.tokens[*index].range.end)
            .unwrap_or(0)
    }

    fn span(&self) -> SourceSpan {
        span_at(self.source, self.offset())
    }

    fn error(&self, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            level: DiagnosticLevel::Error,
            span: self.span(),
            message: message.into(),
        }
    }
}

struct ExpressionParser<'a> {
    source: &'a str,
    tokens: &'a [NativeToken],
    indices: &'a [usize],
    cursor: usize,
}

impl<'a> ExpressionParser<'a> {
    fn new(source: &'a str, tokens: &'a [NativeToken], indices: &'a [usize]) -> Self {
        Self {
            source,
            tokens,
            indices,
            cursor: 0,
        }
    }

    fn parse(&mut self) -> Result<EiyashouExpr, String> {
        self.validate_explicit_grouping()?;
        let expression = self.parse_or()?;
        if self.cursor != self.indices.len() {
            return Err(format!(
                "unexpected token `{}` in expression",
                self.peek().unwrap_or("")
            ));
        }
        Ok(expression)
    }

    fn validate_explicit_grouping(&self) -> Result<(), String> {
        let mut categories = vec![HashSet::new()];
        for index in self.indices {
            let text = &self.source[self.tokens[*index].range.clone()];
            match text {
                "(" | "[" => categories.push(HashSet::new()),
                ")" | "]" => {
                    if categories
                        .pop()
                        .is_some_and(|operators| operators.len() > 1)
                    {
                        return Err("comparison, membership, and logic operators require explicit parentheses when mixed".into());
                    }
                }
                "==" | "!=" | "<" | "<=" | ">" | ">=" => {
                    categories.last_mut().unwrap().insert("comparison");
                }
                "in" => {
                    categories.last_mut().unwrap().insert("membership");
                }
                "and" | "or" => {
                    categories.last_mut().unwrap().insert("logic");
                }
                _ => {}
            }
        }
        if categories.iter().any(|operators| operators.len() > 1) {
            Err("comparison, membership, and logic operators require explicit parentheses when mixed".into())
        } else {
            Ok(())
        }
    }

    fn parse_or(&mut self) -> Result<EiyashouExpr, String> {
        let mut expression = self.parse_and()?;
        while self.eat("or") {
            expression = binary(EiyashouBinaryOp::Or, expression, self.parse_and()?);
        }
        Ok(expression)
    }

    fn parse_and(&mut self) -> Result<EiyashouExpr, String> {
        let mut expression = self.parse_comparison()?;
        while self.eat("and") {
            expression = binary(EiyashouBinaryOp::And, expression, self.parse_comparison()?);
        }
        Ok(expression)
    }

    fn parse_comparison(&mut self) -> Result<EiyashouExpr, String> {
        let expression = self.parse_additive()?;
        let operation = match self.peek() {
            Some("==") => Some(EiyashouBinaryOp::Equal),
            Some("!=") => Some(EiyashouBinaryOp::NotEqual),
            Some("<") => Some(EiyashouBinaryOp::Less),
            Some("<=") => Some(EiyashouBinaryOp::LessEqual),
            Some(">") => Some(EiyashouBinaryOp::Greater),
            Some(">=") => Some(EiyashouBinaryOp::GreaterEqual),
            Some("in") => Some(EiyashouBinaryOp::In),
            _ => None,
        };
        let Some(operation) = operation else {
            return Ok(expression);
        };
        self.cursor += 1;
        let expression = binary(operation, expression, self.parse_additive()?);
        if matches!(
            self.peek(),
            Some("==" | "!=" | "<" | "<=" | ">" | ">=" | "in")
        ) {
            return Err("chained comparisons are not allowed".into());
        }
        Ok(expression)
    }

    fn parse_additive(&mut self) -> Result<EiyashouExpr, String> {
        let mut expression = self.parse_multiplicative()?;
        loop {
            let operation = match self.peek() {
                Some("+") => EiyashouBinaryOp::Add,
                Some("-") => EiyashouBinaryOp::Subtract,
                _ => break,
            };
            self.cursor += 1;
            expression = binary(operation, expression, self.parse_multiplicative()?);
        }
        Ok(expression)
    }

    fn parse_multiplicative(&mut self) -> Result<EiyashouExpr, String> {
        let mut expression = self.parse_unary()?;
        loop {
            let operation = match self.peek() {
                Some("*") => EiyashouBinaryOp::Multiply,
                Some("/") => EiyashouBinaryOp::Divide,
                Some("%") => EiyashouBinaryOp::Remainder,
                _ => break,
            };
            self.cursor += 1;
            expression = binary(operation, expression, self.parse_unary()?);
        }
        Ok(expression)
    }

    fn parse_unary(&mut self) -> Result<EiyashouExpr, String> {
        if self.eat("not") {
            return Ok(EiyashouExpr::Unary {
                op: EiyashouUnaryOp::Not,
                value: Box::new(self.parse_unary()?),
            });
        }
        if self.eat("-") {
            return Ok(EiyashouExpr::Unary {
                op: EiyashouUnaryOp::Negate,
                value: Box::new(self.parse_unary()?),
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<EiyashouExpr, String> {
        let mut expression = self.parse_primary()?;
        loop {
            if self.eat("[") {
                let index = self.parse_or()?;
                if !self.eat("]") {
                    return Err("expected `]` after list index".into());
                }
                expression = EiyashouExpr::Index {
                    list: Box::new(expression),
                    index: Box::new(index),
                };
            } else if self.eat(".") {
                if !self.eat("length") {
                    return Err("only `.length` is valid in an expression".into());
                }
                expression = EiyashouExpr::Length(Box::new(expression));
            } else {
                break;
            }
        }
        Ok(expression)
    }

    fn parse_primary(&mut self) -> Result<EiyashouExpr, String> {
        let Some(index) = self.indices.get(self.cursor).copied() else {
            return Err("expected expression".into());
        };
        let token = &self.tokens[index];
        let text = &self.source[token.range.clone()];
        if token.kind == NativeTokenKind::Number {
            self.cursor += 1;
            if text.contains(['.', 'e', 'E']) {
                let value = text
                    .parse::<f64>()
                    .map_err(|_| format!("invalid number literal `{text}`"))?;
                if !value.is_finite() {
                    return Err("float literals must be finite".into());
                }
                return Ok(EiyashouExpr::Literal(Value::Float(value)));
            }
            let value = text
                .parse::<i64>()
                .map_err(|_| format!("integer literal `{text}` is out of range"))?;
            return Ok(EiyashouExpr::Literal(Value::Int(value)));
        }
        if token.kind == NativeTokenKind::String {
            self.cursor += 1;
            return decode_expression_string(text)
                .map(Value::Str)
                .map(EiyashouExpr::Literal);
        }
        if self.eat("true") {
            return Ok(EiyashouExpr::Literal(Value::Bool(true)));
        }
        if self.eat("false") {
            return Ok(EiyashouExpr::Literal(Value::Bool(false)));
        }
        if self.eat("list") {
            if !self.eat("(") {
                return Err("expected `(` after `list`".into());
            }
            let scalar = match self.peek() {
                Some("bool") => EiyashouScalarType::Bool,
                Some("int") => EiyashouScalarType::Int,
                Some("float") => EiyashouScalarType::Float,
                Some("string") => EiyashouScalarType::String,
                _ => return Err("list type must be bool, int, float, or string".into()),
            };
            self.cursor += 1;
            if !self.eat(")") {
                return Err("expected `)` after list type".into());
            }
            return Ok(EiyashouExpr::EmptyList(scalar));
        }
        if self.eat("[") {
            let mut values = Vec::new();
            if self.eat("]") {
                return Err("empty list literals require `list(type)`".into());
            }
            loop {
                values.push(self.parse_or()?);
                if self.eat("]") {
                    break;
                }
                if !self.eat(",") {
                    return Err("expected `,` or `]` in list literal".into());
                }
                if self.peek() == Some("]") {
                    return Err("trailing commas are not allowed".into());
                }
            }
            return Ok(EiyashouExpr::List(values));
        }
        if self.eat("(") {
            let expression = self.parse_or()?;
            if !self.eat(")") {
                return Err("expected `)` after expression".into());
            }
            return Ok(expression);
        }
        if token.kind == NativeTokenKind::Identifier {
            self.cursor += 1;
            return Ok(EiyashouExpr::Variable(text.to_owned()));
        }
        Err(format!("expected expression, found `{text}`"))
    }

    fn peek(&self) -> Option<&'a str> {
        self.indices
            .get(self.cursor)
            .map(|index| &self.source[self.tokens[*index].range.clone()])
    }

    fn eat(&mut self, expected: &str) -> bool {
        if self.peek() == Some(expected) {
            self.cursor += 1;
            true
        } else {
            false
        }
    }
}

fn binary(op: EiyashouBinaryOp, left: EiyashouExpr, right: EiyashouExpr) -> EiyashouExpr {
    EiyashouExpr::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

#[derive(Debug)]
struct Argument {
    name: Option<String>,
    token_indices: Vec<usize>,
}

fn parse_eiyashou_text(
    source: &str,
    raw: &str,
    source_offset: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> EiyashouText {
    let content = raw
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or("");
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut cursor = 0usize;
    while cursor < content.len() {
        let remainder = &content[cursor..];
        if remainder.starts_with("/${") {
            literal.push_str("${");
            cursor += 3;
            continue;
        }
        if remainder.starts_with("${") {
            if !literal.is_empty() {
                parts.push(EiyashouTextPart::Literal(std::mem::take(&mut literal)));
            }
            let expression_start = cursor + 2;
            let Some(expression_end) = interpolation_end(content, expression_start) else {
                diagnostics.push(error_at(
                    source,
                    source_offset + 1 + cursor,
                    "unterminated `${...}` interpolation",
                ));
                literal.push_str(&content[cursor..]);
                break;
            };
            let expression_source = &content[expression_start..expression_end];
            let (tokens, lex_diagnostics) = lex(expression_source);
            if !lex_diagnostics.is_empty() {
                diagnostics.push(error_at(
                    source,
                    source_offset + 1 + expression_start,
                    "invalid interpolation expression",
                ));
            } else {
                let indices = tokens
                    .iter()
                    .enumerate()
                    .filter_map(|(index, token)| {
                        (!matches!(
                            token.kind,
                            NativeTokenKind::Whitespace | NativeTokenKind::Comment
                        ))
                        .then_some(index)
                    })
                    .collect::<Vec<_>>();
                let mut parser = ExpressionParser::new(expression_source, &tokens, &indices);
                match parser.parse() {
                    Ok(expression) => parts.push(EiyashouTextPart::Expression(expression)),
                    Err(message) => diagnostics.push(error_at(
                        source,
                        source_offset + 1 + expression_start,
                        format!("invalid interpolation: {message}"),
                    )),
                }
            }
            cursor = expression_end + 1;
            continue;
        }
        let character = remainder.chars().next().unwrap();
        if character == '\\' {
            let escape_offset = cursor;
            cursor += 1;
            if cursor >= content.len() {
                diagnostics.push(error_at(
                    source,
                    source_offset + 1 + escape_offset,
                    "unterminated string escape",
                ));
                break;
            }
            let escaped = content[cursor..].chars().next().unwrap();
            match escaped {
                '"' => literal.push('"'),
                '\\' => literal.push('\\'),
                'n' => literal.push('\n'),
                'r' => literal.push('\r'),
                't' => literal.push('\t'),
                other => diagnostics.push(error_at(
                    source,
                    source_offset + 1 + escape_offset,
                    format!("unknown string escape `\\{other}`"),
                )),
            }
            cursor += escaped.len_utf8();
        } else {
            literal.push(character);
            cursor += character.len_utf8();
        }
    }
    if !literal.is_empty() || parts.is_empty() {
        parts.push(EiyashouTextPart::Literal(literal));
    }
    EiyashouText { parts }
}

fn interpolation_end(content: &str, mut cursor: usize) -> Option<usize> {
    let mut string = false;
    let mut escaped = false;
    while cursor < content.len() {
        let character = content[cursor..].chars().next()?;
        if string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                string = false;
            }
        } else if character == '"' {
            string = true;
        } else if character == '}' {
            return Some(cursor);
        }
        cursor += character.len_utf8();
    }
    None
}

fn decode_expression_string(raw: &str) -> Result<String, String> {
    let content = raw
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .ok_or_else(|| "unterminated string literal".to_owned())?;
    if content
        .match_indices("${")
        .any(|(offset, _)| !content[..offset].ends_with('/'))
    {
        return Err("interpolation is only valid in dialogue, narration, and choice text".into());
    }
    let mut output = String::new();
    let mut cursor = 0usize;
    while cursor < content.len() {
        let remainder = &content[cursor..];
        if remainder.starts_with("/${") {
            output.push_str("${");
            cursor += 3;
            continue;
        }
        let character = remainder.chars().next().unwrap();
        if character != '\\' {
            output.push(character);
            cursor += character.len_utf8();
            continue;
        }
        cursor += 1;
        let escaped = content[cursor..]
            .chars()
            .next()
            .ok_or_else(|| "unterminated string escape".to_owned())?;
        match escaped {
            '"' => output.push('"'),
            '\\' => output.push('\\'),
            'n' => output.push('\n'),
            'r' => output.push('\r'),
            't' => output.push('\t'),
            other => return Err(format!("unknown string escape `\\{other}`")),
        }
        cursor += escaped.len_utf8();
    }
    Ok(output)
}

fn parse_transition_text(raw: &str) -> Option<(&str, f32)> {
    let (name, duration) = raw.split_once('(')?;
    let duration = duration.strip_suffix(')')?;
    let seconds = if let Some(value) = duration.strip_suffix("ms") {
        value.parse::<f32>().ok()? / 1000.0
    } else {
        let value = duration.strip_suffix('s')?;
        value.parse::<f32>().ok()?
    };
    Some((name, seconds))
}

fn span_at(source: &str, offset: usize) -> SourceSpan {
    let prefix = &source[..offset.min(source.len())];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix.len() + 1, |(_, tail)| tail.chars().count() + 1);
    SourceSpan { line, column }
}

fn error_at(source: &str, offset: usize, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        level: DiagnosticLevel::Error,
        span: span_at(source, offset),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn errors(scene: &ParsedScene) -> Vec<&str> {
        scene
            .report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.level == DiagnosticLevel::Error)
            .map(|diagnostic| diagnostic.message.as_str())
            .collect()
    }

    #[test]
    fn retains_trivia_ranges_and_parses_multiple_named_scenes() {
        let source = "// heading\nscene opening {\n  \"Hello\"\n}\n\nscene ending { \"Bye\" }\n";
        let document = parse_native_document(source);
        let scenes = parse_native_scenes(source);

        assert!(
            document
                .tokens
                .iter()
                .any(|token| token.kind == NativeTokenKind::Comment)
        );
        assert!(
            document
                .tokens
                .iter()
                .any(|token| token.kind == NativeTokenKind::Whitespace)
        );
        assert_eq!(
            document
                .scenes
                .iter()
                .map(|scene| scene.name.as_str())
                .collect::<Vec<_>>(),
            ["opening", "ending"]
        );
        assert_eq!(
            scenes
                .iter()
                .map(|scene| scene.name.as_deref().unwrap())
                .collect::<Vec<_>>(),
            ["opening", "ending"]
        );
        assert!(scenes.iter().all(|scene| errors(scene).is_empty()));
    }

    #[test]
    fn lowers_dialogue_media_and_strict_video_defaults() {
        let source = r#"
scene opening {
  rin: "Hi", rin_001,
  background(day, transition: fade(300ms)),
  wait(500ms),
  video(opening_movie, skippable: false),
  goto(ending)
}
scene ending { "Done" }
"#;
        let scenes = parse_native_scenes(source);
        assert!(
            scenes.iter().all(|scene| errors(scene).is_empty()),
            "{:?}",
            scenes[0].report.diagnostics
        );
        assert!(matches!(
            scenes[0].report.actions[0],
            Action::EiyashouSay(_)
        ));
        assert!(
            matches!(scenes[0].report.actions[1], Action::ShowBg { transition: Transition::Fade(value), .. } if (value - 0.3).abs() < f32::EPSILON)
        );
        assert!(matches!(
            scenes[0].report.actions[3],
            Action::PlayVideo {
                video: VideoSpec {
                    looped: false,
                    wait_for_finished: true,
                    skippable: false,
                    mode: VideoMode::Fullscreen,
                    ..
                }
            }
        ));
    }

    #[test]
    fn lowers_named_anchor_moves_without_replacing_sprite_content() {
        let scenes = parse_native_scenes(
            "scene a { move(hero, right), move(hero, center, duration: 300ms, easing: ease_out) }",
        );
        assert!(errors(&scenes[0]).is_empty());
        assert!(matches!(
            scenes[0].report.actions[0],
            Action::MoveSprite {
                ref id,
                position,
                duration,
                easing: Easing::Linear,
                blocking: true,
            } if id == "hero" && position == Position::right(0.0) && duration == 0.0
        ));
        assert!(matches!(
            scenes[0].report.actions[1],
            Action::MoveSprite {
                ref id,
                position,
                duration,
                easing: Easing::EaseOut,
                blocking: true,
            } if id == "hero" && position == Position::center(0.0) && (duration - 0.3).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn lowers_explicit_scene_return_to_typed_ir() {
        let scenes = parse_native_scenes("scene a { return }");

        assert!(errors(&scenes[0]).is_empty());
        assert_eq!(scenes[0].report.actions, vec![Action::ReturnScene]);
    }

    #[test]
    fn interpolation_escape_and_typed_interpolation_are_lowered() {
        let escaped =
            parse_native_scenes(r#"scene a { "literal /${amount}; / and $ stay literal" }"#);
        assert!(errors(&escaped[0]).is_empty());
        assert!(
            matches!(&escaped[0].report.actions[0], Action::EiyashouSay(dialogue) if dialogue.text == EiyashouText::literal("literal ${amount}; / and $ stay literal"))
        );

        let interpolation = parse_native_scenes(r#"scene a { let amount = 3, "value ${amount}" }"#);
        assert!(errors(&interpolation[0]).is_empty());
        assert!(matches!(
            &interpolation[0].report.actions[1],
            Action::EiyashouSay(dialogue)
                if matches!(dialogue.text.parts.as_slice(), [EiyashouTextPart::Literal(prefix), EiyashouTextPart::Expression(EiyashouExpr::Variable(name))] if prefix == "value " && name == "amount")
        ));
    }

    #[test]
    fn removed_generic_setting_and_wide_video_parameters_are_errors() {
        let scenes = parse_native_scenes(
            r#"scene a { setting(theme, dark), video(movie, loop: true, wait: false) }"#,
        );
        let messages = errors(&scenes[0]);
        assert!(
            messages
                .iter()
                .any(|message| message.contains("generic `setting"))
        );
        assert!(
            messages
                .iter()
                .any(|message| message.contains("no `loop` or `wait`"))
        );
    }

    #[test]
    fn lowers_typed_variables_lists_control_flow_and_choice_branches() {
        let scenes = parse_native_scenes(
            r#"
scene start {
  let score = 0,
  let items = list(string),
  let removed = "",
  items.append("key"),
  if ((score == 0) and ("key" in items)) {
    score += 1
  } else {
    score = 3
  },
  loop { break },
  choice("Next?") {
    @take "Take" when (score == 1): {
      items.insert(1, "ticket"),
      pop(items, 0, into: removed),
      goto(done)
    },
    @leave "Leave": goto(done)
  }
}
scene done { "${removed}" }
"#,
        );
        let diagnostics = scenes
            .iter()
            .flat_map(|scene| errors(scene))
            .collect::<Vec<_>>();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert!(scenes[0].report.actions.iter().any(|action| matches!(
            action,
            Action::EiyashouList {
                operation: EiyashouListOperation::Insert { .. },
                ..
            }
        )));
        assert!(
            scenes[0]
                .report
                .actions
                .iter()
                .any(|action| matches!(action, Action::EiyashouJumpIf { .. }))
        );
        assert!(scenes[0].report.actions.iter().any(
            |action| matches!(action, Action::EiyashouMenu { choices, .. } if choices.len() == 2)
        ));
    }

    #[test]
    fn rejects_type_errors_duplicate_ids_and_maybe_uninitialized_reads() {
        let scenes = parse_native_scenes(
            r#"
scene start {
  let condition = true,
  if (condition) { let key = "yes" },
  @same "first",
  @same "second",
  condition = 1,
  "${key}"
}
"#,
        );
        let messages = errors(&scenes[0]);
        assert!(
            messages
                .iter()
                .any(|message| message.contains("duplicate stable source id"))
        );
        assert!(
            messages
                .iter()
                .any(|message| message.contains("assignment type"))
        );
        assert!(
            messages
                .iter()
                .any(|message| message.contains("may be uninitialized"))
        );
    }
}
