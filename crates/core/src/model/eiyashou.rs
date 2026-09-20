use serde::{Deserialize, Serialize};

use super::{ChoiceTarget, Rgba, SayOptions, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EiyashouScalarType {
    Bool,
    Int,
    Float,
    String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EiyashouType {
    Scalar(EiyashouScalarType),
    List(EiyashouScalarType),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EiyashouUnaryOp {
    Not,
    Negate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EiyashouBinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
    In,
}

/// Strict, side-effect-free expression IR used only by Eiyashou.
///
/// Compatibility adapters retain their existing string evaluator. This tree
/// keeps Eiyashou's types and short-circuit behavior independent from it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EiyashouExpr {
    Literal(Value),
    Variable(String),
    List(Vec<EiyashouExpr>),
    EmptyList(EiyashouScalarType),
    Unary {
        op: EiyashouUnaryOp,
        value: Box<EiyashouExpr>,
    },
    Binary {
        op: EiyashouBinaryOp,
        left: Box<EiyashouExpr>,
        right: Box<EiyashouExpr>,
    },
    Index {
        list: Box<EiyashouExpr>,
        index: Box<EiyashouExpr>,
    },
    Length(Box<EiyashouExpr>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EiyashouTextPart {
    Literal(String),
    Expression(EiyashouExpr),
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EiyashouText {
    pub parts: Vec<EiyashouTextPart>,
}

impl EiyashouText {
    pub fn literal(value: impl Into<String>) -> Self {
        Self {
            parts: vec![EiyashouTextPart::Literal(value.into())],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EiyashouPlace {
    Variable(String),
    Index {
        variable: String,
        index: EiyashouExpr,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EiyashouAssignOp {
    Replace,
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EiyashouListOperation {
    Append(EiyashouExpr),
    Remove(EiyashouExpr),
    Clear,
    Insert {
        index: EiyashouExpr,
        value: EiyashouExpr,
    },
    Pop {
        index: Option<EiyashouExpr>,
        into: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EiyashouChoice {
    pub text: EiyashouText,
    pub target: ChoiceTarget,
    pub show_when: Option<EiyashouExpr>,
    pub source_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EiyashouDialogue {
    pub speaker: String,
    pub speaker_color: Option<Rgba>,
    pub text: EiyashouText,
    pub options: SayOptions,
    pub source_id: String,
}
