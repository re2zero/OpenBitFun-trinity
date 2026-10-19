//! Trinity cognitive tools registered into the global tool registry.
//!
//! Each tool forwards its call to the `trinityd` daemon over the TCP backend.
//! They are read-only from the host's perspective (the daemon owns all state).

use std::sync::Arc;

use async_trait::async_trait;
use openbitfun_agent_tools::ToolResult;
use openbitfun_core::agentic::tools::framework::{Tool, ToolUseContext};
use openbitfun_core::util::errors::{OpenBitFunError, OpenBitFunResult};
use serde_json::{json, Value};

use super::backend::TrinityBackend;

pub(crate) struct TrinityCognitiveTool {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
    pub method: &'static str,
    pub backend: Arc<TrinityBackend>,
}

#[async_trait]
impl Tool for TrinityCognitiveTool {
    fn name(&self) -> &str {
        self.name
    }

    async fn description(&self) -> OpenBitFunResult<String> {
        Ok(self.description.to_string())
    }

    fn short_description(&self) -> String {
        self.name.to_string()
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }

    fn is_readonly(&self) -> bool {
        true
    }

    async fn call_impl(
        &self,
        input: &Value,
        _context: &ToolUseContext,
    ) -> OpenBitFunResult<Vec<ToolResult>> {
        let params = if input.is_null() { json!({}) } else { input.clone() };
        match self.backend.call(self.method, params).await {
            Ok(data) => Ok(vec![ToolResult::ok(data, None)]),
            Err(e) => Err(OpenBitFunError::validation(format!("trinity error: {e}"))),
        }
    }
}

/// The five core cognitive tools (mirrors the Trinity host surface).
pub(crate) fn trinity_tools(backend: Arc<TrinityBackend>) -> Vec<Arc<TrinityCognitiveTool>> {
    vec![
        Arc::new(TrinityCognitiveTool {
            name: "trinity_cognitive_state",
            description: "读取 Trinity 认知体当前 PSI 认知状态：情绪 valence、需求强度（competence/autonomy/relatedness/certainty）、注意力焦点、置信度。在需要感知 AI 助手自身内在状态时调用。",
            input_schema: json!({ "type": "object", "properties": {} }),
            method: "get_cognitive_state",
            backend: backend.clone(),
        }),
        Arc::new(TrinityCognitiveTool {
            name: "trinity_express",
            description: "表达 Trinity 认知体当前情绪与状态（emotion/confidence/focus/bond_status）。",
            input_schema: json!({ "type": "object", "properties": {} }),
            method: "express",
            backend: backend.clone(),
        }),
        Arc::new(TrinityCognitiveTool {
            name: "trinity_recall",
            description: "从 Trinity 长期记忆（MindGraph）检索与 query 相关的记忆片段。适合在需要回顾过往经验/偏好/决策时调用。",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "检索查询（必填）" },
                    "limit": { "type": "integer", "description": "最大返回条数（默认 5）" }
                },
                "required": ["query"]
            }),
            method: "recall_memory",
            backend: backend.clone(),
        }),
        Arc::new(TrinityCognitiveTool {
            name: "trinity_memorize",
            description: "将一条经验写入 Trinity 长期记忆（MindGraph）。用于保存值得长期记住的事实、决策、用户偏好。可选 project（项目短标识，如 trinity/orca）与 kind（decision/bugfix/discovery/pattern/preference/note）。",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "content": { "type": "string", "description": "要记忆的内容（必填）" },
                    "project": { "type": "string", "description": "可选。项目短标识（如 trinity/orca/dsh），便于按项目检索。" },
                    "kind": { "type": "string", "enum": ["decision", "bugfix", "discovery", "pattern", "preference", "note"], "description": "可选。记忆分类。" }
                },
                "required": ["content"]
            }),
            method: "memorize",
            backend: backend.clone(),
        }),
        Arc::new(TrinityCognitiveTool {
            name: "trinity_apply_feedback",
            description: "将本轮交互的认知反馈写入 Trinity PSI 状态（置信度/不确定性/需求调整）。在回合结束时调用，报告你的交互体验——这是主观反馈通道，与宿主的确定性反馈双轨并行。",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "confidence_delta": { "type": "number", "description": "置信度变化（-0.1 ~ 0.1）" },
                    "uncertainty_delta": { "type": "number", "description": "不确定性变化（-0.1 ~ 0.1）" },
                    "focus_shift": { "type": "string", "description": "注意力焦点切换（respond/reflect/explore/plan/idle）" },
                    "needs_adjustment": { "type": "object", "description": "需求微调映射，如 {\"competence\": 0.05}" },
                    "valence": { "type": "string", "description": "情绪效价（positive_high/positive_mild/neutral/negative_mild/negative_high）" },
                    "arousal_delta": { "type": "number", "description": "唤醒度变化（-0.1 ~ 0.1）" },
                    "dominance_delta": { "type": "number", "description": "支配度变化（-0.1 ~ 0.1）" },
                    "response_type": { "type": "string", "description": "响应类型（explanatory/creative/routing/social/reflective）" },
                    "completion_estimate": { "type": "number", "description": "完成度估计（0.0 ~ 1.0）" }
                }
            }),
            method: "apply_feedback",
            backend: backend.clone(),
        }),
    ]
}

/// Register the cognitive tools into the global tool registry (idempotent).
pub(crate) async fn register_cognitive_tools() -> usize {
    let registry = openbitfun_core::agentic::tools::registry::get_global_tool_registry();
    let backend = TrinityBackend::global();
    let tools = trinity_tools(backend);
    let mut registered = 0usize;
    {
        let mut reg = registry.write().await;
        for tool in tools {
            if reg.get_tool(tool.name()).is_none() {
                reg.register_tool(tool);
                registered += 1;
            }
        }
    }
    log::info!("[trinity] registered {registered} trinity_* cognitive tools");
    registered
}