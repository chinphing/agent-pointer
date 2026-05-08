pub mod builtin;

use crate::models::ToolDef;
use anyhow::Result;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

pub type ToolHandler = Arc<dyn Fn(serde_json::Value) -> Result<String> + Send + Sync>;

pub struct ToolEntry {
    pub def: ToolDef,
    pub handler: ToolHandler,
}

#[derive(Default)]
pub struct ToolRegistry {
    inner: RwLock<HashMap<String, ToolEntry>>,
}

impl ToolRegistry {
    pub fn new() -> Self { Self::default() }

    pub fn register(&self, def: ToolDef, handler: ToolHandler) {
        self.inner.write().insert(def.name.clone(), ToolEntry { def, handler });
    }

    pub fn list_defs(&self) -> Vec<ToolDef> {
        self.inner.read().values().map(|e| e.def.clone()).collect()
    }

    pub fn get_def(&self, name: &str) -> Option<ToolDef> {
        self.inner.read().get(name).map(|e| e.def.clone())
    }

    pub fn invoke(&self, name: &str, args: serde_json::Value) -> Result<String> {
        let g = self.inner.read();
        let entry = g.get(name).ok_or_else(|| anyhow::anyhow!("未注册的工具: {name}"))?;
        let handler = entry.handler.clone();
        drop(g);
        handler(args)
    }

    pub fn openai_tools(&self, allow: &[String]) -> Vec<serde_json::Value> {
        self.inner
            .read()
            .values()
            .filter(|e| allow.is_empty() || allow.contains(&e.def.name))
            .map(|e| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": e.def.name,
                        "description": e.def.description,
                        "parameters": e.def.parameters_schema
                    }
                })
            })
            .collect()
    }
}
