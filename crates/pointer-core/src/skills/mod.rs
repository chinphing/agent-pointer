pub mod builtin;

use crate::models::SkillDef;
use parking_lot::RwLock;
use std::collections::HashMap;

#[derive(Default)]
pub struct SkillRegistry {
    inner: RwLock<HashMap<String, SkillDef>>,
}

impl SkillRegistry {
    pub fn new() -> Self { Self::default() }

    pub fn register(&self, def: SkillDef) {
        self.inner.write().insert(def.id.clone(), def);
    }

    pub fn list(&self) -> Vec<SkillDef> {
        let g = self.inner.read();
        let mut v: Vec<SkillDef> = g.values().cloned().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    pub fn get(&self, id: &str) -> Option<SkillDef> {
        self.inner.read().get(id).cloned()
    }

    /// Resolve enabled skills into (system_prompts, allowed_tool_names)
    pub fn resolve(&self, ids: &[String]) -> (Vec<String>, Vec<String>) {
        let g = self.inner.read();
        let mut prompts = Vec::new();
        let mut tools = Vec::new();
        for id in ids {
            if let Some(s) = g.get(id) {
                prompts.push(format!("【技能：{}】\n{}", s.name, s.system_prompt));
                for t in &s.tool_names {
                    if !tools.contains(t) { tools.push(t.clone()); }
                }
            }
        }
        (prompts, tools)
    }
}
