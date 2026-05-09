use super::{ToolHandler, ToolPrompt, ToolRegistry};
use crate::models::ToolDef;
use anyhow::anyhow;
use std::sync::Arc;

const MATH_PROMPT: &str = include_str!("prompts/math.md");

pub fn register_all(reg: &ToolRegistry) {
    register_calculator(reg);
}

fn register_calculator(reg: &ToolRegistry) {
    let h: ToolHandler = Arc::new(|args| {
        let expr = args
            .get("expression")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("缺少 expression"))?;
        let v = eval_expr(expr).map_err(|e| anyhow!("表达式错误: {e}"))?;
        Ok(serde_json::json!({ "expression": expr, "result": v }).to_string())
    });
    reg.register_with_prompt(
        ToolDef {
            name: "calculator".into(),
            description: "对算术表达式求值，支持 + - * / ( ) 与小数。例如 (3+4)*2.5".into(),
            parameters_schema: serde_json::json!({
                "type":"object",
                "properties":{ "expression":{"type":"string","description":"算术表达式"} },
                "required":["expression"]
            }),
            risk_level: "low".into(),
            requires_approval: false,
        },
        Some(ToolPrompt {
            system_prompt: MATH_PROMPT.into(),
        }),
        h,
    );
}

fn eval_expr(s: &str) -> std::result::Result<f64, String> {
    let toks = tokenize(s)?;
    let rpn = to_rpn(&toks)?;
    eval_rpn(&rpn)
}

#[derive(Debug, Clone)]
enum Tok {
    Num(f64),
    Op(char),
    LParen,
    RParen,
}

fn tokenize(s: &str) -> std::result::Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let str_part: String = chars[start..i].iter().collect();
            let n: f64 = str_part
                .parse()
                .map_err(|e: std::num::ParseFloatError| e.to_string())?;
            out.push(Tok::Num(n));
            continue;
        }
        match c {
            '+' | '-' | '*' | '/' => out.push(Tok::Op(c)),
            '(' => out.push(Tok::LParen),
            ')' => out.push(Tok::RParen),
            _ => return Err(format!("非法字符: {c}")),
        }
        i += 1;
    }
    Ok(out)
}

fn prec(c: char) -> i32 {
    match c {
        '+' | '-' => 1,
        '*' | '/' => 2,
        _ => 0,
    }
}

fn to_rpn(tokens: &[Tok]) -> std::result::Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let mut ops: Vec<Tok> = Vec::new();
    for t in tokens {
        match t {
            Tok::Num(_) => out.push(t.clone()),
            Tok::Op(c) => {
                while let Some(Tok::Op(c2)) = ops.last() {
                    if prec(*c2) >= prec(*c) {
                        out.push(ops.pop().unwrap());
                    } else {
                        break;
                    }
                }
                ops.push(t.clone());
            }
            Tok::LParen => ops.push(t.clone()),
            Tok::RParen => {
                let mut found = false;
                while let Some(top) = ops.pop() {
                    if matches!(top, Tok::LParen) {
                        found = true;
                        break;
                    }
                    out.push(top);
                }
                if !found {
                    return Err("括号不匹配".into());
                }
            }
        }
    }
    while let Some(t) = ops.pop() {
        if matches!(t, Tok::LParen | Tok::RParen) {
            return Err("括号不匹配".into());
        }
        out.push(t);
    }
    Ok(out)
}

fn eval_rpn(rpn: &[Tok]) -> std::result::Result<f64, String> {
    let mut st: Vec<f64> = Vec::new();
    for t in rpn {
        match t {
            Tok::Num(n) => st.push(*n),
            Tok::Op(c) => {
                let b = st.pop().ok_or("缺少操作数")?;
                let a = st.pop().ok_or("缺少操作数")?;
                let v = match c {
                    '+' => a + b,
                    '-' => a - b,
                    '*' => a * b,
                    '/' => {
                        if b == 0.0 {
                            return Err("除零".into());
                        }
                        a / b
                    }
                    _ => return Err("未知运算符".into()),
                };
                st.push(v);
            }
            _ => return Err("非法表达式".into()),
        }
    }
    st.pop().ok_or("空表达式".into())
}
