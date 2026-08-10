use crate::model::{AgentInfo, ModelRef, ProviderInfo, Skill, VcsDiffMode, WorkspaceFile};
use std::collections::BTreeSet;

pub const BUILTIN_TOOL_NAMES: &[&str] = &[
    "apply_patch",
    "bash",
    "edit",
    "execute",
    "glob",
    "grep",
    "lsp",
    "plan_exit",
    "question",
    "read",
    "skill",
    "task",
    "todo",
    "webfetch",
    "websearch",
    "write",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayState {
    Slash {
        selected: usize,
    },
    Model {
        query: String,
        selected: usize,
    },
    Skill {
        query: String,
        selected: usize,
    },
    Agent {
        query: String,
        selected: usize,
    },
    Variant {
        query: String,
        selected: usize,
    },
    Mcp {
        selected: usize,
    },
    CommandPalette {
        query: String,
        selected: usize,
    },
    Mention {
        query: String,
        selected: usize,
        start: usize,
        end: usize,
    },
    Subtask {
        prompt: String,
        selected: usize,
    },
    #[allow(dead_code)]
    PromptOptions {
        selected: usize,
    },
    PromptPanel {
        selected: usize,
    },
    PromptTools {
        selected: usize,
    },
    PromptToolName {
        value: String,
    },
    PromptSystem {
        value: String,
    },
    Theme {
        selected: usize,
    },
    Diagnostics,
    RenameSession {
        value: String,
    },
    DeleteSession {
        session_id: String,
    },
    ArchiveSession {
        session_id: String,
        restore: bool,
    },
    MoveSession {
        session_id: String,
        destination: String,
        move_changes: bool,
    },
    SessionDiff {
        selected: usize,
        scroll: usize,
    },
    VcsDiff {
        mode: VcsDiffMode,
        selected: usize,
        scroll: usize,
    },
    SessionShare {
        url: String,
    },
    AttachFile {
        value: String,
    },
    FilePicker {
        path: String,
        entries: Vec<WorkspaceFile>,
        selected: usize,
        loading: bool,
    },
    Timeline {
        selected: usize,
    },
    ForkSession {
        selected: usize,
    },
    Help,
}

impl OverlayState {
    pub fn selected(&self) -> usize {
        match self {
            Self::Slash { selected }
            | Self::Model { selected, .. }
            | Self::Skill { selected, .. }
            | Self::Agent { selected, .. }
            | Self::Variant { selected, .. }
            | Self::Mcp { selected }
            | Self::CommandPalette { selected, .. }
            | Self::Mention { selected, .. }
            | Self::Subtask { selected, .. }
            | Self::PromptOptions { selected }
            | Self::PromptPanel { selected }
            | Self::PromptTools { selected }
            | Self::Theme { selected }
            | Self::FilePicker { selected, .. }
            | Self::Timeline { selected }
            | Self::ForkSession { selected }
            | Self::SessionDiff { selected, .. }
            | Self::VcsDiff { selected, .. } => *selected,
            Self::RenameSession { .. }
            | Self::DeleteSession { .. }
            | Self::ArchiveSession { .. }
            | Self::MoveSession { .. }
            | Self::SessionShare { .. }
            | Self::PromptToolName { .. }
            | Self::PromptSystem { .. }
            | Self::Diagnostics
            | Self::AttachFile { .. }
            | Self::Help => 0,
        }
    }

    pub fn set_selected(&mut self, selected: usize) {
        match self {
            Self::Slash { selected: current }
            | Self::Model {
                selected: current, ..
            }
            | Self::Skill {
                selected: current, ..
            }
            | Self::Agent {
                selected: current, ..
            }
            | Self::Variant {
                selected: current, ..
            }
            | Self::Mcp { selected: current }
            | Self::CommandPalette {
                selected: current, ..
            }
            | Self::Mention {
                selected: current, ..
            } => *current = selected,
            Self::Subtask {
                selected: current, ..
            }
            | Self::PromptOptions { selected: current }
            | Self::PromptPanel { selected: current }
            | Self::PromptTools { selected: current }
            | Self::Theme { selected: current }
            | Self::FilePicker {
                selected: current, ..
            }
            | Self::Timeline { selected: current }
            | Self::ForkSession { selected: current }
            | Self::SessionDiff {
                selected: current, ..
            }
            | Self::VcsDiff {
                selected: current, ..
            } => *current = selected,
            Self::RenameSession { .. }
            | Self::DeleteSession { .. }
            | Self::ArchiveSession { .. }
            | Self::MoveSession { .. }
            | Self::SessionShare { .. }
            | Self::PromptToolName { .. }
            | Self::PromptSystem { .. }
            | Self::Diagnostics
            | Self::AttachFile { .. }
            | Self::Help => {}
        }
    }
}

pub fn tool_override_names(overrides: &std::collections::HashMap<String, bool>) -> Vec<String> {
    let mut names = BUILTIN_TOOL_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<BTreeSet<_>>();
    names.extend(overrides.keys().cloned());
    names.into_iter().collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOption {
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub model_name: String,
    pub context_limit: u64,
    pub output_limit: u64,
}

impl ModelOption {
    pub fn model_ref(&self) -> ModelRef {
        ModelRef {
            id: self.model_id.clone(),
            provider_id: self.provider_id.clone(),
            variant: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillOption {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentOption {
    pub name: String,
    pub description: String,
    pub native: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantOption {
    pub name: String,
}

pub fn model_options(providers: &[ProviderInfo], query: &str) -> Vec<ModelOption> {
    let mut options = providers
        .iter()
        .flat_map(|provider| {
            provider.models.iter().filter_map(|(key, model)| {
                let model_id = if model.id.is_empty() {
                    key.clone()
                } else {
                    model.id.clone()
                };
                if model_id.is_empty() {
                    return None;
                }
                let provider_name = fallback(&provider.name, &provider.id).to_owned();
                let model_name = fallback(&model.name, &model_id).to_owned();
                let provider_id = fallback(&model.provider_id, &provider.id).to_owned();
                matches_query(
                    query,
                    &[&provider_id, &provider_name, &model_id, &model_name],
                )
                .then_some(ModelOption {
                    provider_id,
                    provider_name,
                    model_id,
                    model_name,
                    context_limit: model.limit.context,
                    output_limit: model.limit.output,
                })
            })
        })
        .collect::<Vec<_>>();

    options.sort_by(|left, right| {
        left.provider_name
            .cmp(&right.provider_name)
            .then_with(|| left.model_name.cmp(&right.model_name))
            .then_with(|| left.provider_id.cmp(&right.provider_id))
            .then_with(|| left.model_id.cmp(&right.model_id))
    });
    options
}

pub fn skill_options(skills: &[Skill], query: &str) -> Vec<SkillOption> {
    let mut options = skills
        .iter()
        .filter(|skill| !skill.name.is_empty())
        .filter_map(|skill| {
            let description = compact_description(&skill.description);
            matches_query(query, &[&skill.name, &description]).then_some(SkillOption {
                name: skill.name.clone(),
                description,
            })
        })
        .collect::<Vec<_>>();
    options.sort_by(|left, right| left.name.cmp(&right.name));
    options
}

pub fn agent_options(agents: &[AgentInfo], query: &str) -> Vec<AgentOption> {
    let mut options = agents
        .iter()
        .filter(|agent| agent.is_selectable())
        .filter_map(|agent| {
            let description = compact_description(&agent.description);
            matches_query(query, &[&agent.name, &description]).then_some(AgentOption {
                name: agent.name.clone(),
                description,
                native: agent.native,
            })
        })
        .collect::<Vec<_>>();
    options.sort_by(|left, right| left.name.cmp(&right.name));
    options
}

pub fn variant_options(
    providers: &[ProviderInfo],
    model: Option<&ModelRef>,
    query: &str,
) -> Vec<VariantOption> {
    let Some(model) = model else {
        return Vec::new();
    };
    let Some(info) = providers
        .iter()
        .find(|provider| provider.id == model.provider_id)
        .and_then(|provider| {
            provider
                .models
                .iter()
                .find_map(|(key, info)| (key == &model.id || info.id == model.id).then_some(info))
        })
    else {
        return Vec::new();
    };
    if info.variants.is_empty() {
        return Vec::new();
    }

    let mut names = std::iter::once("default".to_owned())
        .chain(
            info.variants
                .keys()
                .filter(|name| name.as_str() != "default")
                .cloned(),
        )
        .filter(|name| matches_query(query, &[name]))
        .map(|name| VariantOption { name })
        .collect::<Vec<_>>();
    names.sort_by(
        |left, right| match (left.name == "default", right.name == "default") {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => left.name.cmp(&right.name),
        },
    );
    names
}

fn matches_query(query: &str, values: &[&str]) -> bool {
    let needle = query.trim().to_lowercase();
    needle.is_empty()
        || values
            .iter()
            .any(|value| value.to_lowercase().contains(&needle))
}

fn compact_description(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn fallback<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.is_empty() { fallback } else { value }
}

#[cfg(test)]
mod tests {
    use super::{OverlayState, model_options, skill_options, tool_override_names};
    use crate::model::{AgentInfo, ModelInfo, ModelRef, ProviderInfo, Skill};
    use std::collections::HashMap;

    #[test]
    fn model_options_are_sorted_and_searchable() {
        let providers = vec![
            ProviderInfo {
                id: "provider-b".to_owned(),
                name: "Provider B".to_owned(),
                models: HashMap::from([(
                    "model-z".to_owned(),
                    ModelInfo {
                        id: "model-z".to_owned(),
                        name: "Zeta".to_owned(),
                        ..ModelInfo::default()
                    },
                )]),
            },
            ProviderInfo {
                id: "provider-a".to_owned(),
                name: "Provider A".to_owned(),
                models: HashMap::from([(
                    "model-a".to_owned(),
                    ModelInfo {
                        id: "model-a".to_owned(),
                        name: "Alpha".to_owned(),
                        ..ModelInfo::default()
                    },
                )]),
            },
        ];

        let all = model_options(&providers, "");
        assert_eq!(all[0].model_id, "model-a");
        assert_eq!(all[0].provider_id, "provider-a");
        assert_eq!(model_options(&providers, "zeta")[0].model_id, "model-z");
    }

    #[test]
    fn skill_options_compact_descriptions_and_filter_names() {
        let skills = vec![
            Skill {
                name: "review".to_owned(),
                description: "Review   the   change".to_owned(),
                ..Skill::default()
            },
            Skill {
                name: "deploy".to_owned(),
                description: "Ship it".to_owned(),
                ..Skill::default()
            },
        ];

        let options = skill_options(&skills, "REV");
        assert_eq!(options[0].name, "review");
        assert_eq!(options[0].description, "Review the change");
    }

    #[test]
    fn overlay_selection_is_mutable_without_exposing_variant_fields() {
        let mut overlay = OverlayState::Model {
            query: String::new(),
            selected: 0,
        };
        overlay.set_selected(3);
        assert_eq!(overlay.selected(), 3);
    }

    #[test]
    fn tool_override_names_keep_builtins_and_custom_tools_sorted() {
        let overrides = HashMap::from([("mcp_search".to_owned(), true)]);
        let names = tool_override_names(&overrides);

        assert!(names.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(names.iter().any(|name| name == "bash"));
        assert!(names.iter().any(|name| name == "mcp_search"));
    }

    #[test]
    fn agent_options_hide_subagents_and_filter_descriptions() {
        let agents = vec![
            AgentInfo {
                name: "build".to_owned(),
                description: "Build the project".to_owned(),
                mode: "primary".to_owned(),
                ..AgentInfo::default()
            },
            AgentInfo {
                name: "explore".to_owned(),
                mode: "subagent".to_owned(),
                ..AgentInfo::default()
            },
        ];

        let options = super::agent_options(&agents, "project");
        assert_eq!(options[0].name, "build");
    }

    #[test]
    fn variant_options_always_include_default_and_filter_model_variants() {
        let providers = vec![ProviderInfo {
            id: "provider".to_owned(),
            models: HashMap::from([(
                "model".to_owned(),
                ModelInfo {
                    id: "model".to_owned(),
                    variants: HashMap::from([("fast".to_owned(), serde_json::json!({}))]),
                    ..ModelInfo::default()
                },
            )]),
            ..ProviderInfo::default()
        }];
        let model = ModelRef {
            provider_id: "provider".to_owned(),
            id: "model".to_owned(),
            ..ModelRef::default()
        };

        let all = super::variant_options(&providers, Some(&model), "");
        assert_eq!(
            all.iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            ["default", "fast"]
        );
        assert_eq!(
            super::variant_options(&providers, Some(&model), "fas")[0].name,
            "fast"
        );
    }
}
