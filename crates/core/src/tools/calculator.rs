//! **CalculatorTool** — safe evaluation of mathematical expressions.
//!
//! Supports basic arithmetic: `+`, `-`, `*`, `/`, `^`, parentheses, and
//! common functions (`sqrt`, `abs`, `round`, `min`, `max`).

use crate::agent::tool::{Tool, ToolError, ToolSpec};
use serde_json::{json, Value};

/// A tool that evaluates mathematical expressions safely.
///
/// No access to the filesystem, network, or system state.
/// Uses a hand-written recursive-descent parser.
///
/// # Arguments
/// * `expression` — the math expression string
///
/// # Returns
/// The evaluated result as a number.
pub struct CalculatorTool;

#[async_trait::async_trait]
impl Tool for CalculatorTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "calculator".into(),
            description: "Evaluates a mathematical expression. Supports + - * / ^ () and functions sqrt, abs, round, min, max, sin, cos, pi, e. Returns a numeric result.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "expression": {
                        "type": "string",
                        "description": "The mathematical expression to evaluate"
                    }
                },
                "required": ["expression"]
            }),
            category: crate::agent::tool::ToolCategory::Generic,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let expression = args
            .get("expression")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArgs {
                tool: "calculator".into(),
                message: "missing 'expression' string".into(),
            })?;

        // Tokenize and parse
        let tokens = tokenize(expression)?;
        let result = parse_expression(&tokens)?;

        Ok(json!({
            "expression": expression,
            "result": result
        }))
    }
}

// ── Simple recursive-descent parser ──────────────────────────────────

#[derive(Debug, Clone)]
enum Token {
    Number(f64),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    LParen,
    RParen,
    Comma,
    Ident(String),
}

fn tokenize(input: &str) -> Result<Vec<Token>, ToolError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }

        match chars[i] {
            '+' => tokens.push(Token::Plus),
            '-' => tokens.push(Token::Minus),
            '*' => tokens.push(Token::Star),
            '/' => tokens.push(Token::Slash),
            '^' => tokens.push(Token::Caret),
            '(' => tokens.push(Token::LParen),
            ')' => tokens.push(Token::RParen),
            ',' => tokens.push(Token::Comma),
            '0'..='9' | '.' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let num: f64 = input[start..i].parse().map_err(|_| ToolError::Execution {
                    tool: "calculator".into(),
                    message: format!("invalid number: {}", &input[start..i]),
                })?;
                tokens.push(Token::Number(num));
                continue;
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                tokens.push(Token::Ident(input[start..i].to_string()));
                continue;
            }
            _ => {
                return Err(ToolError::InvalidArgs {
                    tool: "calculator".into(),
                    message: format!("unexpected character: '{}'", chars[i]),
                });
            }
        }
        i += 1;
    }

    Ok(tokens)
}

fn parse_expression(tokens: &[Token]) -> Result<f64, ToolError> {
    let mut pos = 0;
    let result = parse_add_sub(tokens, &mut pos)?;
    if pos < tokens.len() {
        return Err(ToolError::InvalidArgs {
            tool: "calculator".into(),
            message: format!("unexpected token at position {pos}"),
        });
    }
    Ok(result)
}

fn parse_add_sub(tokens: &[Token], pos: &mut usize) -> Result<f64, ToolError> {
    let mut left = parse_mul_div(tokens, pos)?;

    while *pos < tokens.len() {
        match &tokens[*pos] {
            Token::Plus => {
                *pos += 1;
                let right = parse_mul_div(tokens, pos)?;
                left += right;
            }
            Token::Minus => {
                *pos += 1;
                let right = parse_mul_div(tokens, pos)?;
                left -= right;
            }
            _ => break,
        }
    }

    Ok(left)
}

fn parse_mul_div(tokens: &[Token], pos: &mut usize) -> Result<f64, ToolError> {
    let mut left = parse_power(tokens, pos)?;

    while *pos < tokens.len() {
        match &tokens[*pos] {
            Token::Star => {
                *pos += 1;
                let right = parse_power(tokens, pos)?;
                left *= right;
            }
            Token::Slash => {
                *pos += 1;
                let right = parse_power(tokens, pos)?;
                if right == 0.0 {
                    return Err(ToolError::Execution {
                        tool: "calculator".into(),
                        message: "division by zero".into(),
                    });
                }
                left /= right;
            }
            _ => break,
        }
    }

    Ok(left)
}

fn parse_power(tokens: &[Token], pos: &mut usize) -> Result<f64, ToolError> {
    let left = parse_unary(tokens, pos)?;

    if *pos < tokens.len() && matches!(tokens[*pos], Token::Caret) {
        *pos += 1;
        let right = parse_unary(tokens, pos)?;
        return Ok(left.powf(right));
    }

    Ok(left)
}

fn parse_unary(tokens: &[Token], pos: &mut usize) -> Result<f64, ToolError> {
    if *pos >= tokens.len() {
        return Err(ToolError::InvalidArgs {
            tool: "calculator".into(),
            message: "unexpected end of expression".into(),
        });
    }

    match &tokens[*pos] {
        Token::Plus => {
            *pos += 1;
            parse_unary(tokens, pos)
        }
        Token::Minus => {
            *pos += 1;
            let val = parse_unary(tokens, pos)?;
            Ok(-val)
        }
        _ => parse_atom(tokens, pos),
    }
}

fn parse_atom(tokens: &[Token], pos: &mut usize) -> Result<f64, ToolError> {
    if *pos >= tokens.len() {
        return Err(ToolError::InvalidArgs {
            tool: "calculator".into(),
            message: "unexpected end of expression".into(),
        });
    }

    match &tokens[*pos] {
        Token::Number(n) => {
            *pos += 1;
            Ok(*n)
        }
        Token::Ident(name) => {
            *pos += 1;
            // Check for function call: ident (
            if *pos < tokens.len() && matches!(tokens[*pos], Token::LParen) {
                *pos += 1; // consume (
                let mut args = Vec::new();
                if *pos < tokens.len() && !matches!(tokens[*pos], Token::RParen) {
                    args.push(parse_add_sub(tokens, pos)?);
                    while *pos < tokens.len() && matches!(tokens[*pos], Token::Comma) {
                        *pos += 1;
                        args.push(parse_add_sub(tokens, pos)?);
                    }
                }
                if *pos >= tokens.len() || !matches!(tokens[*pos], Token::RParen) {
                    return Err(ToolError::InvalidArgs {
                        tool: "calculator".into(),
                        message: format!("expected ')' after arguments for '{name}'"),
                    });
                }
                *pos += 1; // consume )
                apply_function(name, &args)
            } else {
                // Named constant
                match name.as_str() {
                    "pi" => Ok(std::f64::consts::PI),
                    "e" => Ok(std::f64::consts::E),
                    _ => Err(ToolError::InvalidArgs {
                        tool: "calculator".into(),
                        message: format!("unknown identifier: '{name}'"),
                    }),
                }
            }
        }
        Token::LParen => {
            *pos += 1;
            let val = parse_add_sub(tokens, pos)?;
            if *pos >= tokens.len() || !matches!(tokens[*pos], Token::RParen) {
                return Err(ToolError::InvalidArgs {
                    tool: "calculator".into(),
                    message: "expected ')'".into(),
                });
            }
            *pos += 1;
            Ok(val)
        }
        _ => Err(ToolError::InvalidArgs {
            tool: "calculator".into(),
            message: format!("unexpected token {:?}", tokens[*pos]),
        }),
    }
}

fn apply_function(name: &str, args: &[f64]) -> Result<f64, ToolError> {
    let err = || ToolError::InvalidArgs {
        tool: "calculator".into(),
        message: format!("function '{name}' expects {} arguments, got {}", expected(name), args.len()),
    };

    match name {
        "sqrt" => {
            if args.len() != 1 { return Err(err()); }
            Ok(args[0].sqrt())
        }
        "abs" => {
            if args.len() != 1 { return Err(err()); }
            Ok(args[0].abs())
        }
        "round" => {
            if args.len() != 1 { return Err(err()); }
            Ok(args[0].round())
        }
        "min" => {
            if args.len() != 2 { return Err(err()); }
            Ok(args[0].min(args[1]))
        }
        "max" => {
            if args.len() != 2 { return Err(err()); }
            Ok(args[0].max(args[1]))
        }
        "sin" => {
            if args.len() != 1 { return Err(err()); }
            Ok(args[0].sin())
        }
        "cos" => {
            if args.len() != 1 { return Err(err()); }
            Ok(args[0].cos())
        }
        "ceil" => {
            if args.len() != 1 { return Err(err()); }
            Ok(args[0].ceil())
        }
        "floor" => {
            if args.len() != 1 { return Err(err()); }
            Ok(args[0].floor())
        }
        _ => Err(ToolError::InvalidArgs {
            tool: "calculator".into(),
            message: format!("unknown function: '{name}'"),
        }),
    }
}

fn expected(name: &str) -> usize {
    match name {
        "min" | "max" => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calc(expr: &str) -> f64 {
        let tokens = tokenize(expr).unwrap();
        parse_expression(&tokens).unwrap()
    }

    #[test]
    fn test_simple_addition() {
        assert!((calc("2 + 3") - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_multiplication() {
        assert!((calc("4 * 5") - 20.0).abs() < 1e-10);
    }

    #[test]
    fn test_division() {
        assert!((calc("10 / 4") - 2.5).abs() < 1e-10);
    }

    #[test]
    fn test_power() {
        assert!((calc("2 ^ 3") - 8.0).abs() < 1e-10);
    }

    #[test]
    fn test_precedence() {
        assert!((calc("2 + 3 * 4") - 14.0).abs() < 1e-10);
    }

    #[test]
    fn test_parentheses() {
        assert!((calc("(2 + 3) * 4") - 20.0).abs() < 1e-10);
    }

    #[test]
    fn test_unary_minus() {
        assert!((calc("-5 + 3") - (-2.0)).abs() < 1e-10);
    }

    #[test]
    fn test_sqrt_function() {
        assert!((calc("sqrt(9)") - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_pi_constant() {
        assert!((calc("pi") - std::f64::consts::PI).abs() < 1e-10);
    }

    #[test]
    fn test_min_function() {
        assert!((calc("min(3, 7)") - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_division_by_zero() {
        let tokens = tokenize("1 / 0").unwrap();
        assert!(parse_expression(&tokens).is_err());
    }

    #[test]
    fn test_combined() {
        // sqrt(4) = 2, (3+2)^2 = 25, 2 * 25 = 50
        assert!((calc("sqrt(4) * (3 + 2) ^ 2") - 50.0).abs() < 1e-10);
    }
}
