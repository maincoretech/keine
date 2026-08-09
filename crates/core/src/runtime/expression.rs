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
                Value::Int(value) => Ok(Value::Int(-value)),
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
        let equal = left == right || numeric_pair(&left, &right).is_some_and(|(a, b)| a == b);
        return Ok(Value::Bool(if operator == "==" { equal } else { !equal }));
    }
    if operator == "+" && (matches!(left, Value::Str(_)) || matches!(right, Value::Str(_))) {
        return Ok(Value::Str(left.display() + &right.display()));
    }
    let (left, right) = numeric_pair(&left, &right)
        .ok_or_else(|| format!("operator {operator} requires compatible values"))?;
    match operator {
        "+" => number(left + right),
        "-" => number(left - right),
        "*" => number(left * right),
        "/" if right != 0.0 => Ok(Value::Float(left / right)),
        "/" => Err("division by zero".into()),
        "%" if right != 0.0 => number(left % right),
        "%" => Err("division by zero".into()),
        ">" => Ok(Value::Bool(left > right)),
        ">=" => Ok(Value::Bool(left >= right)),
        "<" => Ok(Value::Bool(left < right)),
        "<=" => Ok(Value::Bool(left <= right)),
        _ => Err(format!("unsupported operator {operator}")),
    }
}

fn numeric_pair(left: &Value, right: &Value) -> Option<(f64, f64)> {
    fn numeric(value: &Value) -> Option<f64> {
        match value {
            Value::Int(value) => Some(*value as f64),
            Value::Float(value) => Some(*value),
            _ => None,
        }
    }
    Some((numeric(left)?, numeric(right)?))
}

fn number(value: f64) -> Result<Value, String> {
    if value.is_finite()
        && value.fract() == 0.0
        && value >= i64::MIN as f64
        && value <= i64::MAX as f64
    {
        Ok(Value::Int(value as i64))
    } else if value.is_finite() {
        Ok(Value::Float(value))
    } else {
        Err("numeric result is not finite".into())
    }
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
}
