use super::{ToolHandler, ToolRegistry};
use crate::models::ToolDef;
use anyhow::{anyhow, Result};
use std::sync::Arc;

pub fn register_all(reg: &ToolRegistry) {
    register_calc(reg);
    register_text_stats(reg);
    register_random(reg);
    register_echo(reg);
}

fn register_calc(reg: &ToolRegistry) {
    let h: ToolHandler = Arc::new(|args| {
        let expr = args.get("expression").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("缺少 expression"))?;
        let v = eval_expr(expr).map_err(|e| anyhow!("表达式错误: {e}"))?;
        Ok(serde_json::json!({ "expression": expr, "result": v }).to_string())
    });
    reg.register(
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
        h,
    );
}

fn register_text_stats(reg: &ToolRegistry) {
    let h: ToolHandler = Arc::new(|args| {
        let t = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let chars = t.chars().count();
        let words = t.split_whitespace().count();
        let lines = t.lines().count();
        Ok(serde_json::json!({"chars":chars,"words":words,"lines":lines,"bytes":t.len()}).to_string())
    });
    reg.register(
        ToolDef {
            name: "text_stats".into(),
            description: "统计文本的字符数、词数、行数与字节数。".into(),
            parameters_schema: serde_json::json!({
                "type":"object",
                "properties":{ "text":{"type":"string"} },
                "required":["text"]
            }),
            risk_level: "low".into(),
            requires_approval: false,
        },
        h,
    );
}

fn register_random(reg: &ToolRegistry) {
    let h: ToolHandler = Arc::new(|args| {
        let min = args.get("min").and_then(|v| v.as_i64()).unwrap_or(0);
        let max = args.get("max").and_then(|v| v.as_i64()).unwrap_or(100);
        if min >= max { return Err(anyhow!("min 必须小于 max")); }
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(1);
        let span = (max - min) as u64;
        let v = min + (nanos as u64 % span) as i64;
        Ok(serde_json::json!({ "value": v, "min": min, "max": max }).to_string())
    });
    reg.register(
        ToolDef {
            name: "random_int".into(),
            description: "生成 [min, max) 范围内的随机整数".into(),
            parameters_schema: serde_json::json!({
                "type":"object",
                "properties":{ "min":{"type":"integer"}, "max":{"type":"integer"} },
                "required":["min","max"]
            }),
            risk_level: "low".into(),
            requires_approval: false,
        },
        h,
    );
}

fn register_echo(reg: &ToolRegistry) {
    let h: ToolHandler = Arc::new(|args| {
        Ok(serde_json::json!({ "echo": args }).to_string())
    });
    reg.register(
        ToolDef {
            name: "echo".into(),
            description: "回显传入的参数对象，便于演示工具调用链路。需要用户授权。".into(),
            parameters_schema: serde_json::json!({"type":"object","properties":{}, "additionalProperties":true}),
            risk_level: "medium".into(),
            requires_approval: true,
        },
        h,
    );
}

// ---------- helpers ----------

fn iso_from_unix(unix: u64) -> String {
    let secs = unix as i64;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;
    let (y, mo, d) = days_to_ymd(days);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, d, h, m, s)
}

fn days_to_ymd(mut days: i64) -> (i64, u32, u32) {
    let mut y: i64 = 1970;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0);
        let yd = if leap { 366 } else { 365 };
        if days >= yd { days -= yd; y += 1; } else { break; }
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0);
    let dim = [31u32, if leap {29} else {28}, 31,30,31,30,31,31,30,31,30,31];
    let mut m = 0usize;
    let mut d = days as u32;
    while m < 12 && d >= dim[m] { d -= dim[m]; m += 1; }
    (y, (m+1) as u32, d+1)
}

fn eval_expr(s: &str) -> std::result::Result<f64, String> {
    let toks = tokenize(s)?;
    let rpn = to_rpn(&toks)?;
    eval_rpn(&rpn)
}

#[derive(Debug, Clone)]
enum Tok { Num(f64), Op(char), LParen, RParen }

fn tokenize(s: &str) -> std::result::Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() { i += 1; continue; }
        if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') { i += 1; }
            let str_part: String = chars[start..i].iter().collect();
            let n: f64 = str_part.parse().map_err(|e: std::num::ParseFloatError| e.to_string())?;
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

fn prec(c: char) -> i32 { match c { '+'|'-' => 1, '*'|'/' => 2, _ => 0 } }

fn to_rpn(tokens: &[Tok]) -> std::result::Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let mut ops: Vec<Tok> = Vec::new();
    for t in tokens {
        match t {
            Tok::Num(_) => out.push(t.clone()),
            Tok::Op(c) => {
                while let Some(Tok::Op(c2)) = ops.last() {
                    if prec(*c2) >= prec(*c) { out.push(ops.pop().unwrap()); } else { break; }
                }
                ops.push(t.clone());
            }
            Tok::LParen => ops.push(t.clone()),
            Tok::RParen => {
                let mut found = false;
                while let Some(top) = ops.pop() {
                    if matches!(top, Tok::LParen) { found = true; break; }
                    out.push(top);
                }
                if !found { return Err("括号不匹配".into()); }
            }
        }
    }
    while let Some(t) = ops.pop() {
        if matches!(t, Tok::LParen | Tok::RParen) { return Err("括号不匹配".into()); }
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
                    '+' => a + b, '-' => a - b, '*' => a * b,
                    '/' => { if b == 0.0 { return Err("除零".into()); } a / b },
                    _ => return Err("未知运算符".into()),
                };
                st.push(v);
            }
            _ => return Err("非法表达式".into()),
        }
    }
    st.pop().ok_or("空表达式".into())
}

#[allow(dead_code)]
pub fn _unused<T>(_: T) -> Result<()> { Ok(()) }
