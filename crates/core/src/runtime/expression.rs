use std::cmp::Ordering;
use std::collections::HashMap;

use crate::Value;

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Value(Value),
    Ident(String),
    Op(&'static str),
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Comma,
}

pub fn evaluate(
    source: &str,
    vars: &HashMap<String, Value>,
    globals: &HashMap<String, Value>,
) -> Result<Value, String> {
    let tokens = tokenize(source)?;
    let mut parser = Parser {
        tokens: &tokens,
        cursor: 0,
        vars,
        globals,
    };
    let value = parser.parse_expression(0)?;
    if parser.cursor != tokens.len() {
        return Err("unexpected trailing expression input".into());
    }
    Ok(value)
}

pub fn interpolate(
    source: &str,
    vars: &HashMap<String, Value>,
    globals: &HashMap<String, Value>,
) -> String {
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0;
    while let Some(relative_open) = source[cursor..].find('{') {
        let open = cursor + relative_open;
        if open > 0 && source.as_bytes()[open - 1] == b'\\' {
            output.push_str(&source[cursor..open - 1]);
            output.push('{');
            cursor = open + 1;
            continue;
        }
        output.push_str(&source[cursor..open]);
        let Some(relative_close) = source[open + 1..].find('}') else {
            output.push_str(&source[open..]);
            return output;
        };
        let close = open + 1 + relative_close;
        let expression = &source[open + 1..close];
        match evaluate(expression.trim(), vars, globals) {
            Ok(value) => output.push_str(&value.display()),
            Err(_) => output.push_str(&source[open..=close]),
        }
        cursor = close + 1;
    }
    output.push_str(&source[cursor..]);
    output
}

fn tokenize(source: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = source.char_indices().peekable();
    while let Some((start, character)) = chars.next() {
        match character {
            value if value.is_whitespace() => {}
            '(' => tokens.push(Token::LeftParen),
            ')' => tokens.push(Token::RightParen),
            '[' => tokens.push(Token::LeftBracket),
            ']' => tokens.push(Token::RightBracket),
            ',' => tokens.push(Token::Comma),
            quote @ ('"' | '\'') => {
                let mut value = String::new();
                let mut terminated = false;
                while let Some((_, character)) = chars.next() {
                    if character == quote {
                        terminated = true;
                        break;
                    }
                    if character == '\\' {
                        if let Some((_, escaped)) = chars.next() {
                            value.push(escaped);
                        }
                    } else {
                        value.push(character);
                    }
                }
                if !terminated {
                    return Err("unterminated string".into());
                }
                tokens.push(Token::Value(Value::Str(value)));
            }
            value if value.is_ascii_digit() || value == '.' => {
                let mut end = start + value.len_utf8();
                while let Some(&(index, next)) = chars.peek() {
                    if !next.is_ascii_digit() && next != '.' {
                        break;
                    }
                    chars.next();
                    end = index + next.len_utf8();
                }
                let raw = &source[start..end];
                if raw.contains('.') {
                    tokens.push(Token::Value(Value::Float(
                        raw.parse().map_err(|_| format!("invalid number {raw}"))?,
                    )));
                } else {
                    tokens.push(Token::Value(Value::Int(
                        raw.parse().map_err(|_| format!("invalid number {raw}"))?,
                    )));
                }
            }
            value if value.is_alphabetic() || matches!(value, '_' | '$') => {
                let mut end = start + value.len_utf8();
                while let Some(&(index, next)) = chars.peek() {
                    if !next.is_alphanumeric() && !matches!(next, '_' | '$' | '.') {
                        break;
                    }
                    chars.next();
                    end = index + next.len_utf8();
                }
                let ident = &source[start..end];
                match ident {
                    "true" => tokens.push(Token::Value(Value::Bool(true))),
                    "false" => tokens.push(Token::Value(Value::Bool(false))),
                    _ => tokens.push(Token::Ident(ident.to_owned())),
                }
            }
            _ => {
                let rest = &source[start..];
                let operator = [
                    "||", "&&", "==", "!=", ">=", "<=", "+", "-", "*", "/", "%", ">", "<", "!",
                ]
                .into_iter()
                .find(|operator| rest.starts_with(operator))
                .ok_or_else(|| format!("unexpected character {character}"))?;
                tokens.push(Token::Op(operator));
                for _ in 1..operator.len() {
                    chars.next();
                }
            }
        }
    }
    Ok(tokens)
}

struct Parser<'a> {
    tokens: &'a [Token],
    cursor: usize,
    vars: &'a HashMap<String, Value>,
    globals: &'a HashMap<String, Value>,
}

impl Parser<'_> {
    fn parse_expression(&mut self, minimum_precedence: u8) -> Result<Value, String> {
        let mut left = self.parse_prefix()?;
        while let Some(Token::Op(operator)) = self.tokens.get(self.cursor) {
            let precedence = precedence(operator);
            if precedence < minimum_precedence {
                break;
            }
            let operator = *operator;
            self.cursor += 1;
            let right = self.parse_expression(precedence + 1)?;
            left = apply_binary(operator, left, right)?;
        }
        Ok(left)
    }

    fn parse_prefix(&mut self) -> Result<Value, String> {
        let token = self
            .tokens
            .get(self.cursor)
            .cloned()
            .ok_or_else(|| "expected expression".to_string())?;
        self.cursor += 1;
        let mut value = match token {
            Token::Value(value) => Ok(value),
            Token::Ident(name) => self
                .vars
                .get(&name)
                .or_else(|| self.globals.get(&name))
                .cloned()
                .ok_or_else(|| format!("unknown variable {name}")),
            Token::Op("!") => Ok(Value::Bool(!self.parse_expression(7)?.truthy())),
            Token::Op("-") => match self.parse_expression(7)? {
                Value::Int(value) => value
                    .checked_neg()
                    .map(Value::Int)
                    .ok_or_else(|| "integer negation overflow".into()),
                Value::Float(value) => Ok(Value::Float(-value)),
                _ => Err("unary minus requires a number".into()),
            },
            Token::LeftParen => {
                let value = self.parse_expression(0)?;
                self.expect(Token::RightParen)?;
                Ok(value)
            }
            Token::LeftBracket => {
                let mut values = Vec::new();
                if self.tokens.get(self.cursor) != Some(&Token::RightBracket) {
                    loop {
                        values.push(self.parse_expression(0)?);
                        if self.tokens.get(self.cursor) != Some(&Token::Comma) {
                            break;
                        }
                        self.cursor += 1;
                    }
                }
                self.expect(Token::RightBracket)?;
                Ok(Value::Array(values))
            }
            _ => Err("unexpected expression token".into()),
        }?;
        while self.tokens.get(self.cursor) == Some(&Token::LeftBracket) {
            self.cursor += 1;
            let index = self.parse_expression(0)?;
            self.expect(Token::RightBracket)?;
            let Value::Int(index) = index else {
                return Err("array index must be an integer".into());
            };
            let Value::Array(values) = value else {
                return Err("indexing requires an array".into());
            };
            value = values
                .get(usize::try_from(index).map_err(|_| "array index cannot be negative")?)
                .cloned()
                .ok_or_else(|| "array index out of bounds".to_string())?;
        }
        Ok(value)
    }

    fn expect(&mut self, expected: Token) -> Result<(), String> {
        if self.tokens.get(self.cursor) != Some(&expected) {
            return Err(format!("expected {expected:?}"));
        }
        self.cursor += 1;
        Ok(())
    }
}

fn precedence(operator: &str) -> u8 {
    match operator {
        "||" => 1,
        "&&" => 2,
        "==" | "!=" => 3,
        ">" | ">=" | "<" | "<=" => 4,
        "+" | "-" => 5,
        "*" | "/" | "%" => 6,
        _ => 0,
    }
}

fn apply_binary(operator: &str, left: Value, right: Value) -> Result<Value, String> {
    if operator == "&&" || operator == "||" {
        return Ok(Value::Bool(if operator == "&&" {
            left.truthy() && right.truthy()
        } else {
            left.truthy() || right.truthy()
        }));
    }
    if operator == "==" || operator == "!=" {
        let equal = left == right || numeric_equal(&left, &right).unwrap_or(false);
        return Ok(Value::Bool(if operator == "==" { equal } else { !equal }));
    }
    if operator == "+" && (matches!(left, Value::Str(_)) || matches!(right, Value::Str(_))) {
        return Ok(Value::Str(left.display() + &right.display()));
    }
    let (left, right) = numeric_pair(&left, &right)
        .ok_or_else(|| format!("operator {operator} requires compatible values"))?;
    if let (Numeric::Int(left), Numeric::Int(right)) = (left, right) {
        return match operator {
            "+" => checked_integer(left.checked_add(right), "addition"),
            "-" => checked_integer(left.checked_sub(right), "subtraction"),
            "*" => checked_integer(left.checked_mul(right), "multiplication"),
            "/" if right != 0 => divide_integers(left, right),
            "%" if right == -1 => Ok(Value::Int(0)),
            "%" if right != 0 => checked_integer(left.checked_rem(right), "remainder"),
            "/" | "%" => Err("division by zero".into()),
            ">" => Ok(Value::Bool(left > right)),
            ">=" => Ok(Value::Bool(left >= right)),
            "<" => Ok(Value::Bool(left < right)),
            "<=" => Ok(Value::Bool(left <= right)),
            _ => Err(format!("unsupported operator {operator}")),
        };
    }
    if matches!(operator, ">" | ">=" | "<" | "<=") {
        let ordering = numeric_cmp(left, right);
        let result = match operator {
            ">" => ordering == Some(Ordering::Greater),
            ">=" => matches!(ordering, Some(Ordering::Greater | Ordering::Equal)),
            "<" => ordering == Some(Ordering::Less),
            "<=" => matches!(ordering, Some(Ordering::Less | Ordering::Equal)),
            _ => unreachable!(),
        };
        return Ok(Value::Bool(result));
    }
    let (left, right) = (left.as_f64(), right.as_f64());
    match operator {
        "+" => number(left + right),
        "-" => number(left - right),
        "*" => number(left * right),
        "/" if right != 0.0 => finite_float(left / right),
        "%" if right != 0.0 => number(left % right),
        "/" | "%" => Err("division by zero".into()),
        _ => Err(format!("unsupported operator {operator}")),
    }
}

#[derive(Clone, Copy)]
enum Numeric {
    Int(i64),
    Float(f64),
}

impl Numeric {
    fn as_f64(self) -> f64 {
        match self {
            Self::Int(value) => value as f64,
            Self::Float(value) => value,
        }
    }
}

fn numeric_pair(left: &Value, right: &Value) -> Option<(Numeric, Numeric)> {
    fn numeric(value: &Value) -> Option<Numeric> {
        match value {
            Value::Int(value) => Some(Numeric::Int(*value)),
            Value::Float(value) => Some(Numeric::Float(*value)),
            _ => None,
        }
    }
    Some((numeric(left)?, numeric(right)?))
}

fn numeric_equal(left: &Value, right: &Value) -> Option<bool> {
    let (left, right) = numeric_pair(left, right)?;
    Some(numeric_cmp(left, right)? == Ordering::Equal)
}

fn numeric_cmp(left: Numeric, right: Numeric) -> Option<Ordering> {
    match (left, right) {
        (Numeric::Int(left), Numeric::Int(right)) => Some(left.cmp(&right)),
        (Numeric::Float(left), Numeric::Float(right)) => left.partial_cmp(&right),
        (Numeric::Int(left), Numeric::Float(right)) => compare_int_float(left, right),
        (Numeric::Float(left), Numeric::Int(right)) => {
            compare_int_float(right, left).map(Ordering::reverse)
        }
    }
}

fn compare_int_float(integer: i64, float: f64) -> Option<Ordering> {
    const I64_UPPER_EXCLUSIVE: f64 = 9_223_372_036_854_775_808.0;
    if float.is_nan() {
        return None;
    }
    if float >= I64_UPPER_EXCLUSIVE {
        return Some(Ordering::Less);
    }
    if float < i64::MIN as f64 {
        return Some(Ordering::Greater);
    }
    let truncated = float as i64;
    match integer.cmp(&truncated) {
        Ordering::Equal if float.fract() > 0.0 => Some(Ordering::Less),
        Ordering::Equal if float.fract() < 0.0 => Some(Ordering::Greater),
        ordering => Some(ordering),
    }
}

fn checked_integer(value: Option<i64>, operation: &str) -> Result<Value, String> {
    value
        .map(Value::Int)
        .ok_or_else(|| format!("integer {operation} overflow"))
}

fn divide_integers(left: i64, right: i64) -> Result<Value, String> {
    let quotient = left
        .checked_div(right)
        .ok_or_else(|| "integer division overflow".to_owned())?;
    if left.checked_rem(right) == Some(0) {
        Ok(Value::Int(quotient))
    } else {
        finite_float(left as f64 / right as f64)
    }
}

fn finite_float(value: f64) -> Result<Value, String> {
    value
        .is_finite()
        .then_some(Value::Float(value))
        .ok_or_else(|| "numeric result is not finite".into())
}

fn number(value: f64) -> Result<Value, String> {
    Value::from_finite_f64(value).ok_or_else(|| "numeric result is not finite".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_precedence_variables_and_arrays() {
        let vars = HashMap::from([("score".into(), Value::Int(4))]);
        let globals = HashMap::new();
        assert_eq!(
            evaluate("score * 2 + 1", &vars, &globals),
            Ok(Value::Int(9))
        );
        assert_eq!(
            evaluate("score >= 4 && true", &vars, &globals),
            Ok(Value::Bool(true))
        );
        assert_eq!(
            evaluate("[score, 'ok']", &vars, &globals),
            Ok(Value::Array(vec![Value::Int(4), Value::Str("ok".into())]))
        );
        assert_eq!(
            evaluate("[score, 9][1]", &vars, &globals),
            Ok(Value::Int(9))
        );
    }

    #[test]
    fn interpolates_and_preserves_unknown_or_escaped_values() {
        let vars = HashMap::from([("name".into(), Value::Str("Crab".into()))]);
        assert_eq!(
            interpolate("Hi {name}, {missing}, \\{literal}", &vars, &HashMap::new()),
            "Hi Crab, {missing}, {literal}"
        );
    }

    #[test]
    fn borrowed_scanner_preserves_unicode_and_unclosed_input() {
        let vars = HashMap::from([
            ("名字".into(), Value::Str("慧音".into())),
            ("count".into(), Value::Int(2)),
        ]);
        assert_eq!(
            interpolate(
                "你好，{名字}：\\{原样}，{count >= 2}，末尾{",
                &vars,
                &HashMap::new()
            ),
            "你好，慧音：{原样}，true，末尾{"
        );
        assert_eq!(
            evaluate("名字 == '慧音'", &vars, &HashMap::new()),
            Ok(Value::Bool(true))
        );
    }

    #[test]
    fn integer_arithmetic_and_comparisons_remain_exact_above_f64_precision() {
        let vars = HashMap::new();
        let globals = HashMap::new();

        assert_eq!(
            evaluate("9007199254740993 + 1", &vars, &globals),
            Ok(Value::Int(9_007_199_254_740_994))
        );
        assert_eq!(
            evaluate("9007199254740993 > 9007199254740992", &vars, &globals),
            Ok(Value::Bool(true))
        );
        assert_eq!(
            evaluate("9007199254740993 == 9007199254740992.0", &vars, &globals),
            Ok(Value::Bool(false))
        );
        assert_eq!(
            evaluate("9007199254740993 / 1", &vars, &globals),
            Ok(Value::Int(9_007_199_254_740_993))
        );
        assert!(evaluate("9223372036854775807 + 1", &vars, &globals).is_err());
    }
}
