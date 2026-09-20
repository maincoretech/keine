use std::cmp::Ordering;
use std::collections::HashMap;

use crate::{
    EiyashouAssignOp, EiyashouBinaryOp, EiyashouExpr, EiyashouListOperation, EiyashouPlace,
    EiyashouText, EiyashouTextPart, EiyashouUnaryOp, Value,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EiyashouRuntimeError {
    UnknownVariable,
    TypeMismatch,
    DivisionByZero,
    IntegerOverflow,
    NonFiniteFloat,
    IndexOutOfBounds,
    EmptyList,
    MissingListValue,
}

pub fn evaluate(
    expression: &EiyashouExpr,
    variables: &HashMap<String, Value>,
) -> Result<Value, EiyashouRuntimeError> {
    match expression {
        EiyashouExpr::Literal(value) => Ok(value.clone()),
        EiyashouExpr::Variable(name) => variables
            .get(name)
            .cloned()
            .ok_or(EiyashouRuntimeError::UnknownVariable),
        EiyashouExpr::List(items) => {
            let mut values = items
                .iter()
                .map(|item| evaluate(item, variables))
                .collect::<Result<Vec<_>, _>>()?;
            if values.iter().any(|value| matches!(value, Value::Float(_))) {
                for value in &mut values {
                    if let Value::Int(integer) = value {
                        *value = Value::Float(*integer as f64);
                    }
                }
            }
            Ok(Value::Array(values))
        }
        EiyashouExpr::EmptyList(_) => Ok(Value::Array(Vec::new())),
        EiyashouExpr::Unary { op, value } => {
            let value = evaluate(value, variables)?;
            match (op, value) {
                (EiyashouUnaryOp::Not, Value::Bool(value)) => Ok(Value::Bool(!value)),
                (EiyashouUnaryOp::Negate, Value::Int(value)) => value
                    .checked_neg()
                    .map(Value::Int)
                    .ok_or(EiyashouRuntimeError::IntegerOverflow),
                (EiyashouUnaryOp::Negate, Value::Float(value)) => finite(-value),
                _ => Err(EiyashouRuntimeError::TypeMismatch),
            }
        }
        EiyashouExpr::Binary { op, left, right } => {
            let left = evaluate(left, variables)?;
            if *op == EiyashouBinaryOp::And {
                let Value::Bool(left) = left else {
                    return Err(EiyashouRuntimeError::TypeMismatch);
                };
                return if left {
                    strict_bool(evaluate(right, variables)?)
                } else {
                    Ok(Value::Bool(false))
                };
            }
            if *op == EiyashouBinaryOp::Or {
                let Value::Bool(left) = left else {
                    return Err(EiyashouRuntimeError::TypeMismatch);
                };
                return if left {
                    Ok(Value::Bool(true))
                } else {
                    strict_bool(evaluate(right, variables)?)
                };
            }
            let right = evaluate(right, variables)?;
            apply_binary(*op, left, right)
        }
        EiyashouExpr::Index { list, index } => {
            let Value::Array(values) = evaluate(list, variables)? else {
                return Err(EiyashouRuntimeError::TypeMismatch);
            };
            let index = index_value(evaluate(index, variables)?)?;
            values
                .get(index)
                .cloned()
                .ok_or(EiyashouRuntimeError::IndexOutOfBounds)
        }
        EiyashouExpr::Length(list) => {
            let Value::Array(values) = evaluate(list, variables)? else {
                return Err(EiyashouRuntimeError::TypeMismatch);
            };
            i64::try_from(values.len())
                .map(Value::Int)
                .map_err(|_| EiyashouRuntimeError::IntegerOverflow)
        }
    }
}

pub fn render_text(
    text: &EiyashouText,
    variables: &HashMap<String, Value>,
) -> Result<String, EiyashouRuntimeError> {
    let mut output = String::new();
    for part in &text.parts {
        match part {
            EiyashouTextPart::Literal(value) => output.push_str(value),
            EiyashouTextPart::Expression(expression) => match evaluate(expression, variables)? {
                Value::Array(_) => return Err(EiyashouRuntimeError::TypeMismatch),
                value => output.push_str(&value.display()),
            },
        }
    }
    Ok(output)
}

pub fn assign(
    variables: &mut HashMap<String, Value>,
    target: &EiyashouPlace,
    expression: &EiyashouExpr,
    operation: EiyashouAssignOp,
    initialize_once: bool,
) -> Result<(), EiyashouRuntimeError> {
    if let EiyashouPlace::Variable(name) = target {
        if initialize_once && variables.contains_key(name) {
            return Ok(());
        }
        let value = evaluate(expression, variables)?;
        if initialize_once {
            variables.insert(name.clone(), value);
            return Ok(());
        }
        let current = variables
            .get(name)
            .cloned()
            .ok_or(EiyashouRuntimeError::UnknownVariable)?;
        let value = assign_value(current, value, operation)?;
        variables.insert(name.clone(), value);
        return Ok(());
    }

    let EiyashouPlace::Index { variable, index } = target else {
        unreachable!();
    };
    if initialize_once {
        return Err(EiyashouRuntimeError::TypeMismatch);
    }
    let index = index_value(evaluate(index, variables)?)?;
    let right = evaluate(expression, variables)?;
    let list = variables
        .get_mut(variable)
        .ok_or(EiyashouRuntimeError::UnknownVariable)?;
    let Value::Array(values) = list else {
        return Err(EiyashouRuntimeError::TypeMismatch);
    };
    let current = values
        .get(index)
        .cloned()
        .ok_or(EiyashouRuntimeError::IndexOutOfBounds)?;
    values[index] = assign_value(current, right, operation)?;
    Ok(())
}

pub fn mutate_list(
    variables: &mut HashMap<String, Value>,
    variable: &str,
    operation: &EiyashouListOperation,
) -> Result<bool, EiyashouRuntimeError> {
    match operation {
        EiyashouListOperation::Pop { index, into } => {
            let index = index
                .as_ref()
                .map(|index| evaluate(index, variables).and_then(index_value))
                .transpose()?;
            let target = variables
                .get(into)
                .cloned()
                .ok_or(EiyashouRuntimeError::UnknownVariable)?;
            let list = variables
                .get_mut(variable)
                .ok_or(EiyashouRuntimeError::UnknownVariable)?;
            let Value::Array(values) = list else {
                return Err(EiyashouRuntimeError::TypeMismatch);
            };
            if values.is_empty() {
                return Err(EiyashouRuntimeError::EmptyList);
            }
            let index = index.unwrap_or(values.len() - 1);
            if index >= values.len() {
                return Err(EiyashouRuntimeError::IndexOutOfBounds);
            }
            ensure_same_value_type(&target, &values[index])?;
            let value = values.remove(index);
            variables.insert(into.clone(), value);
            Ok(true)
        }
        operation => {
            let evaluated = match operation {
                EiyashouListOperation::Append(value) | EiyashouListOperation::Remove(value) => {
                    Some(evaluate(value, variables)?)
                }
                EiyashouListOperation::Insert { value, .. } => Some(evaluate(value, variables)?),
                EiyashouListOperation::Clear | EiyashouListOperation::Pop { .. } => None,
            };
            let index = match operation {
                EiyashouListOperation::Insert { index, .. } => {
                    Some(index_value(evaluate(index, variables)?)?)
                }
                _ => None,
            };
            let list = variables
                .get_mut(variable)
                .ok_or(EiyashouRuntimeError::UnknownVariable)?;
            let Value::Array(values) = list else {
                return Err(EiyashouRuntimeError::TypeMismatch);
            };
            match operation {
                EiyashouListOperation::Append(_) => {
                    let value = evaluated.ok_or(EiyashouRuntimeError::MissingListValue)?;
                    ensure_list_element_type(values, &value)?;
                    values.push(value);
                    Ok(true)
                }
                EiyashouListOperation::Remove(_) => {
                    let value = evaluated.ok_or(EiyashouRuntimeError::MissingListValue)?;
                    ensure_list_element_type(values, &value)?;
                    if let Some(index) = values
                        .iter()
                        .position(|candidate| value_equal(candidate, &value))
                    {
                        values.remove(index);
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                }
                EiyashouListOperation::Clear => {
                    values.clear();
                    Ok(true)
                }
                EiyashouListOperation::Insert { .. } => {
                    let value = evaluated.ok_or(EiyashouRuntimeError::MissingListValue)?;
                    ensure_list_element_type(values, &value)?;
                    let index = index.expect("insert has an index");
                    if index > values.len() {
                        return Err(EiyashouRuntimeError::IndexOutOfBounds);
                    }
                    values.insert(index, value);
                    Ok(true)
                }
                EiyashouListOperation::Pop { .. } => unreachable!(),
            }
        }
    }
}

fn assign_value(
    current: Value,
    right: Value,
    operation: EiyashouAssignOp,
) -> Result<Value, EiyashouRuntimeError> {
    if operation == EiyashouAssignOp::Replace {
        ensure_same_value_type(&current, &right)?;
        return Ok(promote_for_target(&current, right));
    }
    let operator = match operation {
        EiyashouAssignOp::Add => EiyashouBinaryOp::Add,
        EiyashouAssignOp::Subtract => EiyashouBinaryOp::Subtract,
        EiyashouAssignOp::Multiply => EiyashouBinaryOp::Multiply,
        EiyashouAssignOp::Divide => EiyashouBinaryOp::Divide,
        EiyashouAssignOp::Remainder => EiyashouBinaryOp::Remainder,
        EiyashouAssignOp::Replace => unreachable!(),
    };
    apply_binary(operator, current, right)
}

fn apply_binary(
    operation: EiyashouBinaryOp,
    left: Value,
    right: Value,
) -> Result<Value, EiyashouRuntimeError> {
    use EiyashouBinaryOp as Op;
    match operation {
        Op::Equal | Op::NotEqual => {
            ensure_same_value_type(&left, &right)?;
            let equal = value_equal(&left, &right);
            Ok(Value::Bool(if operation == Op::Equal {
                equal
            } else {
                !equal
            }))
        }
        Op::And | Op::Or => Err(EiyashouRuntimeError::TypeMismatch),
        Op::In => {
            let Value::Array(values) = right else {
                return Err(EiyashouRuntimeError::TypeMismatch);
            };
            if let Some(first) = values.first() {
                ensure_same_value_type(first, &left)?;
            }
            Ok(Value::Bool(
                values.iter().any(|value| value_equal(value, &left)),
            ))
        }
        Op::Add if matches!((&left, &right), (Value::Str(_), Value::Str(_))) => {
            let (Value::Str(left), Value::Str(right)) = (left, right) else {
                unreachable!();
            };
            Ok(Value::Str(left + &right))
        }
        Op::Less | Op::LessEqual | Op::Greater | Op::GreaterEqual => {
            let ordering = numeric_compare(&left, &right)?;
            Ok(Value::Bool(match operation {
                Op::Less => ordering == Ordering::Less,
                Op::LessEqual => ordering != Ordering::Greater,
                Op::Greater => ordering == Ordering::Greater,
                Op::GreaterEqual => ordering != Ordering::Less,
                _ => unreachable!(),
            }))
        }
        Op::Divide => {
            let (left, right) = numeric_f64(&left, &right)?;
            if right == 0.0 {
                return Err(EiyashouRuntimeError::DivisionByZero);
            }
            finite(left / right)
        }
        Op::Remainder => {
            let (Value::Int(left), Value::Int(right)) = (left, right) else {
                return Err(EiyashouRuntimeError::TypeMismatch);
            };
            if right == 0 {
                return Err(EiyashouRuntimeError::DivisionByZero);
            }
            left.checked_rem(right)
                .map(Value::Int)
                .ok_or(EiyashouRuntimeError::IntegerOverflow)
        }
        Op::Add | Op::Subtract | Op::Multiply => numeric_arithmetic(operation, left, right),
    }
}

fn numeric_arithmetic(
    operation: EiyashouBinaryOp,
    left: Value,
    right: Value,
) -> Result<Value, EiyashouRuntimeError> {
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => match operation {
            EiyashouBinaryOp::Add => left.checked_add(right),
            EiyashouBinaryOp::Subtract => left.checked_sub(right),
            EiyashouBinaryOp::Multiply => left.checked_mul(right),
            _ => unreachable!(),
        }
        .map(Value::Int)
        .ok_or(EiyashouRuntimeError::IntegerOverflow),
        (left, right) => {
            let (left, right) = numeric_f64(&left, &right)?;
            finite(match operation {
                EiyashouBinaryOp::Add => left + right,
                EiyashouBinaryOp::Subtract => left - right,
                EiyashouBinaryOp::Multiply => left * right,
                _ => unreachable!(),
            })
        }
    }
}

fn finite(value: f64) -> Result<Value, EiyashouRuntimeError> {
    value
        .is_finite()
        .then_some(Value::Float(value))
        .ok_or(EiyashouRuntimeError::NonFiniteFloat)
}

fn strict_bool(value: Value) -> Result<Value, EiyashouRuntimeError> {
    match value {
        Value::Bool(value) => Ok(Value::Bool(value)),
        _ => Err(EiyashouRuntimeError::TypeMismatch),
    }
}

fn index_value(value: Value) -> Result<usize, EiyashouRuntimeError> {
    let Value::Int(value) = value else {
        return Err(EiyashouRuntimeError::TypeMismatch);
    };
    usize::try_from(value).map_err(|_| EiyashouRuntimeError::IndexOutOfBounds)
}

fn numeric_f64(left: &Value, right: &Value) -> Result<(f64, f64), EiyashouRuntimeError> {
    let value = |value: &Value| match value {
        Value::Int(value) => Some(*value as f64),
        Value::Float(value) => Some(*value),
        _ => None,
    };
    value(left)
        .zip(value(right))
        .ok_or(EiyashouRuntimeError::TypeMismatch)
}

fn numeric_compare(left: &Value, right: &Value) -> Result<Ordering, EiyashouRuntimeError> {
    let (left, right) = numeric_f64(left, right)?;
    left.partial_cmp(&right)
        .ok_or(EiyashouRuntimeError::NonFiniteFloat)
}

fn value_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Int(left), Value::Float(right)) => *left as f64 == *right,
        (Value::Float(left), Value::Int(right)) => *left == *right as f64,
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| value_equal(left, right))
        }
        _ => left == right,
    }
}

fn ensure_list_element_type(values: &[Value], value: &Value) -> Result<(), EiyashouRuntimeError> {
    values
        .first()
        .map_or(Ok(()), |first| ensure_same_value_type(first, value))
}

fn ensure_same_value_type(left: &Value, right: &Value) -> Result<(), EiyashouRuntimeError> {
    let compatible = matches!(
        (left, right),
        (Value::Int(_), Value::Int(_))
            | (Value::Int(_), Value::Float(_))
            | (Value::Float(_), Value::Int(_))
            | (Value::Float(_), Value::Float(_))
            | (Value::Str(_), Value::Str(_))
            | (Value::Bool(_), Value::Bool(_))
            | (Value::Array(_), Value::Array(_))
    );
    compatible
        .then_some(())
        .ok_or(EiyashouRuntimeError::TypeMismatch)
}

fn promote_for_target(target: &Value, value: Value) -> Value {
    match (target, value) {
        (Value::Float(_), Value::Int(value)) => Value::Float(value as f64),
        (_, value) => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn integer(value: i64) -> EiyashouExpr {
        EiyashouExpr::Literal(Value::Int(value))
    }

    #[test]
    fn boolean_operators_short_circuit() {
        let expression = EiyashouExpr::Binary {
            op: EiyashouBinaryOp::And,
            left: Box::new(EiyashouExpr::Literal(Value::Bool(false))),
            right: Box::new(EiyashouExpr::Index {
                list: Box::new(EiyashouExpr::List(Vec::new())),
                index: Box::new(integer(0)),
            }),
        };

        assert_eq!(
            evaluate(&expression, &HashMap::new()),
            Ok(Value::Bool(false))
        );
    }

    #[test]
    fn integer_overflow_and_real_division_fail_or_promote_exactly() {
        let overflow = EiyashouExpr::Binary {
            op: EiyashouBinaryOp::Add,
            left: Box::new(integer(i64::MAX)),
            right: Box::new(integer(1)),
        };
        assert_eq!(
            evaluate(&overflow, &HashMap::new()),
            Err(EiyashouRuntimeError::IntegerOverflow)
        );

        let division = EiyashouExpr::Binary {
            op: EiyashouBinaryOp::Divide,
            left: Box::new(integer(5)),
            right: Box::new(integer(2)),
        };
        assert_eq!(evaluate(&division, &HashMap::new()), Ok(Value::Float(2.5)));
    }
}
