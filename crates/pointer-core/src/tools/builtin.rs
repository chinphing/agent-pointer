use super::{ToolHandler, ToolRegistry};
use crate::models::ToolDef;
use crate::skills::SkillRegistry;
use anyhow::{anyhow, Result};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TERMINAL_DEFAULT_TIMEOUT_MS: u64 = 30_000;
const TERMINAL_MAX_TIMEOUT_MS: u64 = 120_000;
const TERMINAL_DEFAULT_MAX_OUTPUT_BYTES: usize = 20_000;
const TERMINAL_MAX_OUTPUT_BYTES: usize = 200_000;

pub fn register_all(reg: &ToolRegistry) {
    register_calc(reg);
    register_text_stats(reg);
    register_random(reg);
    register_echo(reg);
    register_terminal(reg);
}

pub fn register_skill_tools(reg: &ToolRegistry, skills: Arc<SkillRegistry>) {
    register_load_skill_instructions(reg, skills.clone());
    register_read_skill_resource(reg, skills);
}

fn register_calc(reg: &ToolRegistry) {
    let h: ToolHandler = Arc::new(|args| {
        let expr = args
            .get("expression")
            .and_then(|v| v.as_str())
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
        Ok(
            serde_json::json!({"chars":chars,"words":words,"lines":lines,"bytes":t.len()})
                .to_string(),
        )
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
        if min >= max {
            return Err(anyhow!("min 必须小于 max"));
        }
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(1);
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
    let h: ToolHandler = Arc::new(|args| Ok(serde_json::json!({ "echo": args }).to_string()));
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

fn register_terminal(reg: &ToolRegistry) {
    let h: ToolHandler = Arc::new(run_terminal_command);
    reg.register(
        ToolDef {
            name: "terminal".into(),
            description: "跨平台执行终端命令并返回 stdout、stderr、退出码和耗时。高风险工具，必须经过用户授权。".into(),
            parameters_schema: serde_json::json!({
                "type":"object",
                "properties":{
                    "command":{"type":"string","description":"要执行的终端命令。Windows 使用 cmd /C，macOS/Linux 使用 sh -lc。"},
                    "cwd":{"type":"string","description":"可选工作目录。必须是已存在的目录。"},
                    "timeoutMs":{"type":"integer","description":"可选超时时间，默认 30000，最大 120000。"},
                    "maxOutputBytes":{"type":"integer","description":"可选最大输出字节数，默认 20000，最大 200000。stdout 和 stderr 分别截断。"}
                },
                "required":["command"]
            }),
            risk_level: "high".into(),
            requires_approval: true,
        },
        h,
    );
}

fn register_load_skill_instructions(reg: &ToolRegistry, skills: Arc<SkillRegistry>) {
    let h: ToolHandler = Arc::new(move |args| {
        let id = args
            .get("skill_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("缺少 skill_id"))?;
        skills.load_instructions(id)
    });
    reg.register(
        ToolDef {
            name: "load_skill_instructions".into(),
            description: "加载指定 Skill 的第二层 SKILL.md 正文说明。仅当第一层 description 判断该 Skill 与当前任务相关时调用。".into(),
            parameters_schema: serde_json::json!({
                "type":"object",
                "properties":{ "skill_id":{"type":"string","description":"要加载的 Skill id"} },
                "required":["skill_id"]
            }),
            risk_level: "low".into(),
            requires_approval: false,
        },
        h,
    );
}

fn register_read_skill_resource(reg: &ToolRegistry, skills: Arc<SkillRegistry>) {
    let h: ToolHandler = Arc::new(move |args| {
        let id = args
            .get("skill_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("缺少 skill_id"))?;
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("缺少 path"))?;
        skills.read_resource(id, path)
    });
    reg.register(
        ToolDef {
            name: "read_skill_resource".into(),
            description: "读取指定 Skill 目录中 references、assets 或 scripts 下的第三层资源文件。仅在 SKILL.md 正文要求参考该文件时调用，不会执行脚本。".into(),
            parameters_schema: serde_json::json!({
                "type":"object",
                "properties":{
                    "skill_id":{"type":"string","description":"Skill id"},
                    "path":{"type":"string","description":"资源相对路径，例如 references/api-guide.md"}
                },
                "required":["skill_id", "path"]
            }),
            risk_level: "low".into(),
            requires_approval: false,
        },
        h,
    );
}

// ---------- helpers ----------

fn run_terminal_command(args: serde_json::Value) -> Result<String> {
    let command = args
        .get("command")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow!("缺少 command"))?;
    let cwd = parse_terminal_cwd(args.get("cwd"))?;
    let timeout_ms = args
        .get("timeoutMs")
        .and_then(|v| v.as_u64())
        .unwrap_or(TERMINAL_DEFAULT_TIMEOUT_MS)
        .clamp(1_000, TERMINAL_MAX_TIMEOUT_MS);
    let max_output_bytes = args
        .get("maxOutputBytes")
        .and_then(|v| v.as_u64())
        .unwrap_or(TERMINAL_DEFAULT_MAX_OUTPUT_BYTES as u64)
        .min(TERMINAL_MAX_OUTPUT_BYTES as u64) as usize;

    let (shell, mut cmd) = terminal_shell_command(command);
    if let Some(dir) = &cwd {
        cmd.current_dir(dir);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let started = Instant::now();
    let mut child = cmd.spawn().map_err(|e| anyhow!("启动终端命令失败: {e}"))?;
    let mut timed_out = false;

    loop {
        if let Some(_status) = child.try_wait()? {
            break;
        }
        if started.elapsed() >= Duration::from_millis(timeout_ms) {
            timed_out = true;
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let output = child
        .wait_with_output()
        .map_err(|e| anyhow!("读取终端命令输出失败: {e}"))?;
    let duration_ms = started.elapsed().as_millis() as u64;
    let (stdout, stdout_truncated) = truncate_output(&output.stdout, max_output_bytes);
    let (stderr, stderr_truncated) = truncate_output(&output.stderr, max_output_bytes);

    Ok(serde_json::json!({
        "command": command,
        "cwd": cwd.map(|p| p.display().to_string()).unwrap_or_else(|| std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default()),
        "shell": shell,
        "exitCode": output.status.code(),
        "success": output.status.success() && !timed_out,
        "timedOut": timed_out,
        "durationMs": duration_ms,
        "stdout": stdout,
        "stderr": stderr,
        "stdoutTruncated": stdout_truncated,
        "stderrTruncated": stderr_truncated,
        "maxOutputBytes": max_output_bytes
    })
    .to_string())
}

pub struct TerminalStreamingResult {
    pub exit_code: Option<i32>,
    pub success: bool,
    pub timed_out: bool,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

pub fn run_terminal_command_streaming(
    args: serde_json::Value,
    on_output: impl Fn(&str) + Send,
) -> Result<TerminalStreamingResult> {
    let command = args
        .get("command")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow!("缺少 command"))?;
    let cwd = parse_terminal_cwd(args.get("cwd"))?;
    let timeout_ms = args
        .get("timeoutMs")
        .and_then(|v| v.as_u64())
        .unwrap_or(TERMINAL_DEFAULT_TIMEOUT_MS)
        .clamp(1_000, TERMINAL_MAX_TIMEOUT_MS);
    let max_output_bytes = args
        .get("maxOutputBytes")
        .and_then(|v| v.as_u64())
        .unwrap_or(TERMINAL_DEFAULT_MAX_OUTPUT_BYTES as u64)
        .min(TERMINAL_MAX_OUTPUT_BYTES as u64) as usize;

    let (_shell, mut cmd) = terminal_shell_command(command);
    if let Some(dir) = &cwd {
        cmd.current_dir(dir);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let started = Instant::now();
    let mut child = cmd.spawn().map_err(|e| anyhow!("启动终端命令失败: {e}"))?;

    let stdout_pipe = child.stdout.take().ok_or_else(|| anyhow!("无法获取 stdout"))?;
    let stderr_pipe = child.stderr.take().ok_or_else(|| anyhow!("无法获取 stderr"))?;

    let mut stdout_reader = BufReader::new(stdout_pipe);
    let mut stderr_reader = BufReader::new(stderr_pipe);

    let mut stdout_buf = String::new();
    let mut stderr_buf = String::new();
    let mut timed_out = false;

    loop {
        let mut line = String::new();
        match stdout_reader.read_line(&mut line) {
            Ok(0) | Err(_) => {}
            Ok(_) => {
                on_output(&line);
                stdout_buf.push_str(&line);
            }
        }

        line.clear();
        match stderr_reader.read_line(&mut line) {
            Ok(0) | Err(_) => {}
            Ok(_) => {
                on_output(&line);
                stderr_buf.push_str(&line);
            }
        }

        if let Some(status) = child.try_wait()? {
            drain_remaining_output(&mut stdout_reader, &mut stdout_buf, &on_output);
            drain_remaining_output_stderr(&mut stderr_reader, &mut stderr_buf, &on_output);
            if !status.success() {
                let code = status.code().unwrap_or(-1);
                on_output(&format!("\n进程退出，退出码: {code}\n"));
            }
            break;
        }

        if started.elapsed() >= Duration::from_millis(timeout_ms) {
            timed_out = true;
            let _ = child.kill();
            drain_remaining_output(&mut stdout_reader, &mut stdout_buf, &on_output);
            drain_remaining_output_stderr(&mut stderr_reader, &mut stderr_buf, &on_output);
            on_output("\n进程已超时，已终止执行\n");
            break;
        }

        std::thread::sleep(Duration::from_millis(50));
    }

    let duration_ms = started.elapsed().as_millis() as u64;
    let (stdout, stdout_truncated) = truncate_output(stdout_buf.as_bytes(), max_output_bytes);
    let (stderr, stderr_truncated) = truncate_output(stderr_buf.as_bytes(), max_output_bytes);

    Ok(TerminalStreamingResult {
        exit_code: None,
        success: !timed_out,
        timed_out,
        duration_ms,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    })
}

fn drain_remaining_output(
    reader: &mut BufReader<std::process::ChildStdout>,
    buf: &mut String,
    on_output: &impl Fn(&str),
) {
    let mut line = String::new();
    while let Ok(n) = reader.read_line(&mut line) {
        if n == 0 {
            break;
        }
        on_output(&line);
        buf.push_str(&line);
        line.clear();
    }
}

fn drain_remaining_output_stderr(
    reader: &mut BufReader<std::process::ChildStderr>,
    buf: &mut String,
    on_output: &impl Fn(&str),
) {
    let mut line = String::new();
    while let Ok(n) = reader.read_line(&mut line) {
        if n == 0 {
            break;
        }
        on_output(&line);
        buf.push_str(&line);
        line.clear();
    }
}

fn parse_terminal_cwd(value: Option<&serde_json::Value>) -> Result<Option<PathBuf>> {
    let Some(raw) = value.and_then(|v| v.as_str()).map(str::trim) else {
        return Ok(None);
    };
    if raw.is_empty() {
        return Ok(None);
    }
    let path = PathBuf::from(raw);
    if !path.exists() {
        return Err(anyhow!("cwd 不存在: {raw}"));
    }
    if !path.is_dir() {
        return Err(anyhow!("cwd 不是目录: {raw}"));
    }
    Ok(Some(path))
}

#[cfg(windows)]
fn terminal_shell_command(command: &str) -> (&'static str, Command) {
    let mut cmd = Command::new("cmd");
    cmd.arg("/C").arg(command);
    ("cmd /C", cmd)
}

#[cfg(not(windows))]
fn terminal_shell_command(command: &str) -> (&'static str, Command) {
    let mut cmd = Command::new("sh");
    cmd.arg("-lc").arg(command);
    ("sh -lc", cmd)
}

fn truncate_output(bytes: &[u8], max_bytes: usize) -> (String, bool) {
    if bytes.len() <= max_bytes {
        return (String::from_utf8_lossy(bytes).to_string(), false);
    }
    let mut end = max_bytes.min(bytes.len());
    while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
        end -= 1;
    }
    let mut text = String::from_utf8_lossy(&bytes[..end]).to_string();
    text.push_str("\n...[output truncated]");
    (text, true)
}

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
        if days >= yd {
            days -= yd;
            y += 1;
        } else {
            break;
        }
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0);
    let dim = [
        31u32,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0usize;
    let mut d = days as u32;
    while m < 12 && d >= dim[m] {
        d -= dim[m];
        m += 1;
    }
    (y, (m + 1) as u32, d + 1)
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

#[allow(dead_code)]
pub fn _unused<T>(_: T) -> Result<()> {
    Ok(())
}
