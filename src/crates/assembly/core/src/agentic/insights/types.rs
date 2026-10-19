use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============ Stage 1: Data Collection ============

/// Compact session transcript built from PersistenceManager data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTranscript {
    pub session_id: String,
    pub agent_type: String,
    pub session_name: String,
    pub workspace_path: Option<String>,
    /// For facet cache fingerprinting (`SessionSummary.last_activity_at`).
    #[serde(default)]
    pub last_activity_unix_secs: u64,
    pub duration_minutes: u64,
    pub message_count: u32,
    pub turn_count: u32,
    pub created_at: String,
    /// Compact text transcript ([User]: ... [Tool: xxx] [Assistant]: ...)
    pub transcript: String,
    pub tool_names: Vec<String>,
    pub has_errors: bool,
}

/// Basic statistics accumulated during data collection (pre-AI)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BaseStats {
    pub total_sessions: u32,
    pub total_messages: u32,
    pub total_turns: u32,
    pub total_duration_minutes: u64,
    #[serde(default)]
    pub total_duration_millis: u64,
    pub first_session_at: Option<String>,
    pub last_session_at: Option<String>,
    pub tool_usage: HashMap<String, u32>,
    pub tool_errors: HashMap<String, u32>,
    pub hour_counts: HashMap<u32, u32>,
    pub agent_types: HashMap<String, u32>,
    /// Raw response time intervals in seconds (intermediate, not serialized to report)
    #[serde(skip)]
    pub response_times_raw: Vec<f64>,
    #[serde(default)]
    pub response_time_buckets: HashMap<String, u32>,
    #[serde(default)]
    pub median_response_time_secs: Option<f64>,
    #[serde(default)]
    pub avg_response_time_secs: Option<f64>,
    #[serde(default)]
    pub total_lines_added: usize,
    #[serde(default)]
    pub total_lines_removed: usize,
    #[serde(default)]
    pub total_files_modified: usize,
    /// Language labels inferred from edited file paths (Edit/Write); drives aggregate `languages`.
    #[serde(default)]
    pub languages_by_files: HashMap<String, u32>,
    #[serde(default)]
    pub session_usage: InsightsSessionUsage,
}

// ============ Stage 2: Facet Extraction (AI) ============

/// AI-extracted facets per session (aligned with Claude Code)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionFacet {
    pub session_id: String,
    pub underlying_goal: String,
    pub goal_categories: HashMap<String, u32>,
    /// fully_achieved | partially_achieved | abandoned | unknown
    pub outcome: String,
    pub user_satisfaction_counts: HashMap<String, u32>,
    pub claude_helpfulness: String,
    pub session_type: String,
    pub friction_counts: HashMap<String, u32>,
    pub friction_detail: String,
    pub primary_success: String,
    pub brief_summary: String,
    /// Optional; not used for report language charts (those use file-extension stats from Edit/Write).
    #[serde(default)]
    pub languages_used: Vec<String>,
    #[serde(default)]
    pub user_instructions: Vec<String>,
}

// ============ Stage 3: Aggregation ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateRange {
    pub start: String,
    pub end: String,
}

/// Aggregated data from all sessions (Rust-side computation)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightsAggregate {
    pub sessions: u32,
    pub analyzed: u32,
    pub date_range: DateRange,
    pub messages: u32,
    pub hours: f32,
    pub top_tools: Vec<(String, u32)>,
    pub top_goals: Vec<(String, u32)>,
    pub outcomes: HashMap<String, u32>,
    pub satisfaction: HashMap<String, u32>,
    pub friction: HashMap<String, u32>,
    pub success: HashMap<String, u32>,
    /// Counts by language label from edited file paths (Edit/Write), not from facet extraction.
    pub languages: HashMap<String, u32>,
    pub session_summaries: Vec<String>,
    pub friction_details: Vec<String>,
    pub user_instructions: Vec<String>,
    pub session_types: HashMap<String, u32>,
    pub tool_errors: HashMap<String, u32>,
    pub hour_counts: HashMap<u32, u32>,
    pub agent_types: HashMap<String, u32>,
    pub msgs_per_day: f32,
    #[serde(default)]
    pub response_time_buckets: HashMap<String, u32>,
    #[serde(default)]
    pub median_response_time_secs: Option<f64>,
    #[serde(default)]
    pub avg_response_time_secs: Option<f64>,
    #[serde(default)]
    pub total_lines_added: usize,
    #[serde(default)]
    pub total_lines_removed: usize,
    #[serde(default)]
    pub total_files_modified: usize,
    #[serde(default)]
    pub session_usage: InsightsSessionUsage,
}

// ============ Stage 4: AI Analysis Results ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtAGlance {
    pub whats_working: String,
    pub whats_hindering: String,
    pub quick_wins: String,
    pub looking_ahead: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionStyle {
    pub narrative: String,
    pub key_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectArea {
    pub name: String,
    pub session_count: u32,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BigWin {
    pub title: String,
    pub description: String,
    pub impact: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrictionCategory {
    pub category: String,
    pub count: u32,
    pub description: String,
    pub examples: Vec<String>,
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MdAddition {
    pub section: String,
    pub content: String,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureRecommendation {
    pub feature: String,
    pub description: String,
    pub example_usage: String,
    pub benefit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsagePattern {
    pub pattern: String,
    pub description: String,
    #[serde(default)]
    pub detail: String,
    pub suggested_prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightsSuggestions {
    /// Reports written before the product rename persist this key as
    /// `bitfun_md_additions`; without the alias those saved reports fail to
    /// deserialize and vanish from the history list.
    #[serde(alias = "bitfun_md_additions")]
    pub openbitfun_md_additions: Vec<MdAddition>,
    pub features_to_try: Vec<FeatureRecommendation>,
    pub usage_patterns: Vec<UsagePattern>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HorizonWorkflow {
    pub title: String,
    pub whats_possible: String,
    pub how_to_try: String,
    #[serde(default)]
    pub copyable_prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunEnding {
    pub headline: String,
    pub detail: String,
}

// ============ Stage 5: Final Report ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightsStats {
    pub total_hours: f32,
    pub msgs_per_day: f32,
    pub top_tools: Vec<(String, u32)>,
    pub top_goals: Vec<(String, u32)>,
    pub outcomes: HashMap<String, u32>,
    pub satisfaction: HashMap<String, u32>,
    pub session_types: HashMap<String, u32>,
    pub languages: HashMap<String, u32>,
    pub hour_counts: HashMap<u32, u32>,
    pub agent_types: HashMap<String, u32>,
    #[serde(default)]
    pub response_time_buckets: HashMap<String, u32>,
    #[serde(default)]
    pub median_response_time_secs: Option<f64>,
    #[serde(default)]
    pub avg_response_time_secs: Option<f64>,
    #[serde(default)]
    pub friction: HashMap<String, u32>,
    #[serde(default)]
    pub success: HashMap<String, u32>,
    #[serde(default)]
    pub tool_errors: HashMap<String, u32>,
    #[serde(default)]
    pub total_lines_added: usize,
    #[serde(default)]
    pub total_lines_removed: usize,
    #[serde(default)]
    pub total_files_modified: usize,
}

/// The final insights report (shared between backend and frontend)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightsReport {
    pub generated_at: u64,
    pub date_range: DateRange,
    pub total_sessions: u32,
    pub analyzed_sessions: u32,
    pub total_messages: u32,
    pub days_covered: u32,

    #[serde(default)]
    pub session_usage: InsightsSessionUsage,
    #[serde(default)]
    pub generation_usage: InsightsGenerationUsage,
    #[serde(default)]
    pub generation_models: Vec<String>,

    pub stats: InsightsStats,

    pub at_a_glance: AtAGlance,
    pub interaction_style: InteractionStyle,
    pub project_areas: Vec<ProjectArea>,
    #[serde(default)]
    pub wins_intro: String,
    pub big_wins: Vec<BigWin>,
    #[serde(default)]
    pub friction_intro: String,
    pub friction_categories: Vec<FrictionCategory>,
    pub suggestions: InsightsSuggestions,
    #[serde(default)]
    pub horizon_intro: String,
    pub on_the_horizon: Vec<HorizonWorkflow>,
    pub fun_ending: Option<FunEnding>,

    /// Path to the generated HTML file
    pub html_report_path: Option<String>,
}

/// Token usage accumulated from the persisted turns included in this report.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct InsightsSessionUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub turns_with_usage: u32,
    pub output_reported_turns: u32,
    pub total_turns: u32,
}

/// Token usage reported by model calls made while generating one insights report.
///
/// Providers may omit usage metadata, so callers should compare
/// `reported_model_calls` with `model_calls` before treating the totals as complete.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct InsightsGenerationUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub reasoning_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub model_calls: u32,
    pub reported_model_calls: u32,
}

/// Metadata for listing saved reports
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightsReportMeta {
    pub generated_at: u64,
    pub total_sessions: u32,
    pub analyzed_sessions: u32,
    pub date_range: DateRange,
    pub path: String,
    #[serde(default)]
    pub total_messages: u32,
    #[serde(default)]
    pub days_covered: u32,
    #[serde(default)]
    pub total_hours: f32,
    #[serde(default)]
    pub top_goals: Vec<String>,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub session_usage: InsightsSessionUsage,
    #[serde(default)]
    pub generation_usage: InsightsGenerationUsage,
    #[serde(default)]
    pub generation_models: Vec<String>,
}

// ============ API Request/Response ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateInsightsRequest {
    pub days: Option<u32>,
    pub model_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A report saved before the product rename: the OPENBITFUN.md additions
    /// key is persisted as `bitfun_md_additions`.
    const LEGACY_REPORT: &str = r#"{
        "generated_at": 1785725436,
        "date_range": {
            "start": "2026-08-01T16:31:29.115+00:00",
            "end": "2026-08-03T00:44:44.204+00:00"
        },
        "total_sessions": 6,
        "analyzed_sessions": 6,
        "total_messages": 243,
        "days_covered": 3,
        "generation_models": ["deepseek-v4-flash-free"],
        "stats": {
            "total_hours": 1.1754669,
            "msgs_per_day": 81.0,
            "top_tools": [["ExecCommand", 15]],
            "top_goals": [["身份询问", 10]],
            "outcomes": { "fully_achieved": 6 },
            "satisfaction": { "satisfied": 2 },
            "session_types": { "quick_question": 3 },
            "languages": { "Rust": 13 },
            "hour_counts": { "0": 1 },
            "agent_types": { "agentic": 2 }
        },
        "at_a_glance": {
            "whats_working": "working",
            "whats_hindering": "hindering",
            "quick_wins": "quick",
            "looking_ahead": "ahead"
        },
        "interaction_style": {
            "narrative": "narrative",
            "key_patterns": ["pattern"]
        },
        "project_areas": [
            { "name": "pi-plugin", "session_count": 1, "description": "align" }
        ],
        "big_wins": [
            { "title": "scope", "description": "d", "impact": "i" }
        ],
        "friction_categories": [
            {
                "category": "verify-first",
                "count": 2,
                "description": "d",
                "examples": ["e"],
                "suggestion": "s"
            }
        ],
        "suggestions": {
            "bitfun_md_additions": [
                {
                    "section": "提交规范",
                    "content": "提交前必须运行 git status",
                    "rationale": "只提交本次修改的文件"
                }
            ],
            "features_to_try": [
                {
                    "feature": "Skills",
                    "description": "d",
                    "example_usage": "e",
                    "benefit": "b"
                }
            ],
            "usage_patterns": [
                {
                    "pattern": "p",
                    "description": "d",
                    "detail": "det",
                    "suggested_prompt": "sp"
                }
            ]
        },
        "on_the_horizon": [
            {
                "title": "t",
                "whats_possible": "w",
                "how_to_try": "h",
                "copyable_prompt": "cp"
            }
        ]
    }"#;

    /// Upgrade compatibility: a report saved under the old key must still load,
    /// and re-saving it keeps the current key.
    #[test]
    fn legacy_reports_still_carry_the_md_additions() {
        let report: InsightsReport =
            serde_json::from_str(LEGACY_REPORT).expect("legacy report deserializes");

        assert_eq!(report.suggestions.openbitfun_md_additions.len(), 1);
        assert_eq!(
            report.suggestions.openbitfun_md_additions[0].section,
            "提交规范"
        );
        assert_eq!(report.suggestions.features_to_try.len(), 1);
        assert_eq!(report.suggestions.usage_patterns.len(), 1);

        let value = serde_json::to_value(&report).expect("report serializes");
        let suggestions = value["suggestions"]
            .as_object()
            .expect("suggestions serialize as an object");
        assert!(suggestions.contains_key("openbitfun_md_additions"));
        assert!(!suggestions.contains_key("bitfun_md_additions"));
    }

    /// The current shape keeps working (the alias must not break the new key).
    #[test]
    fn current_reports_read_the_openbitfun_md_additions() {
        let current = LEGACY_REPORT.replace("bitfun_md_additions", "openbitfun_md_additions");
        let report: InsightsReport =
            serde_json::from_str(&current).expect("current report deserializes");
        assert_eq!(report.suggestions.openbitfun_md_additions.len(), 1);
    }
}
