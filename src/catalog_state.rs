use crate::model::{
    AgentInfo, CommandInfo, ModelRef, ProviderCatalog, ProviderInfo, ReferenceInfo, Skill,
    WorkspaceFile,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::warn;

const MAX_RECENT_MODELS: usize = 10;
const RECENT_MODELS_FILE: &str = "recent_models.json";

#[derive(Debug, Clone, Deserialize, Serialize)]
struct RecentModelEntry {
    id: String,
    #[serde(rename = "providerID")]
    provider_id: String,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct RecentModelsFile {
    #[serde(default)]
    models: Vec<RecentModelEntry>,
}

pub struct CatalogState {
    pub providers: Vec<ProviderInfo>,
    pub provider_defaults: HashMap<String, String>,
    pub skills: Vec<Skill>,
    pub commands: Vec<CommandInfo>,
    pub agents: Vec<AgentInfo>,
    pub references: Vec<ReferenceInfo>,
    pub workspace_files: Vec<WorkspaceFile>,
    pub server_workspace_files: Vec<WorkspaceFile>,
    pub server_file_query: Option<String>,
    pub selected_model: Option<ModelRef>,
    pub recent_models: Vec<ModelRef>,
    pub selected_agent: Option<String>,
    recent_models_path: Option<PathBuf>,
}

impl Default for CatalogState {
    fn default() -> Self {
        Self::with_recent_models_path(None)
    }
}

impl CatalogState {
    pub fn persistent() -> Self {
        Self::with_recent_models_path(Self::default_recent_models_path())
    }

    fn with_recent_models_path(path: Option<PathBuf>) -> Self {
        Self {
            providers: Vec::new(),
            provider_defaults: HashMap::new(),
            skills: Vec::new(),
            commands: Vec::new(),
            agents: Vec::new(),
            references: Vec::new(),
            workspace_files: Vec::new(),
            server_workspace_files: Vec::new(),
            server_file_query: None,
            selected_model: None,
            recent_models: load_recent_models(path.as_deref()),
            selected_agent: None,
            recent_models_path: path,
        }
    }

    fn default_recent_models_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("", "", "opencode-tui-rust")
            .map(|dirs| dirs.data_dir().join(RECENT_MODELS_FILE))
    }

    pub fn replace_providers(&mut self, catalog: ProviderCatalog) {
        self.providers = catalog.providers;
        self.provider_defaults = catalog.default;
    }

    pub fn replace_skills(&mut self, skills: Vec<Skill>) {
        self.skills = skills;
    }

    pub fn replace_commands(&mut self, commands: Vec<CommandInfo>) {
        self.commands = commands;
    }

    pub fn replace_agents(&mut self, agents: Vec<AgentInfo>) {
        self.agents = agents;
    }

    pub fn replace_references(&mut self, references: Vec<ReferenceInfo>) {
        self.references = references;
    }

    pub fn replace_workspace_files(&mut self, files: Vec<WorkspaceFile>) {
        self.workspace_files = files;
    }

    pub fn begin_server_file_search(&mut self, query: &str) -> bool {
        if self.server_file_query.as_deref() == Some(query) {
            return false;
        }
        self.server_file_query = Some(query.to_owned());
        true
    }

    pub fn replace_server_workspace_files(&mut self, query: String, files: Vec<WorkspaceFile>) {
        self.server_file_query = Some(query);
        self.server_workspace_files = files;
    }

    pub fn clear_server_file_search(&mut self, query: &str) {
        if self.server_file_query.as_deref() == Some(query) {
            self.server_file_query = None;
            self.server_workspace_files.clear();
        }
    }

    pub fn mention_files(&self) -> Vec<WorkspaceFile> {
        let mut seen = HashSet::new();
        self.server_workspace_files
            .iter()
            .chain(self.workspace_files.iter())
            .filter(|file| seen.insert(file.path.clone()))
            .cloned()
            .collect()
    }

    pub fn select_model(&mut self, model: ModelRef) {
        self.recent_models
            .retain(|recent| recent.provider_id != model.provider_id || recent.id != model.id);
        self.recent_models.insert(
            0,
            ModelRef {
                id: model.id.clone(),
                provider_id: model.provider_id.clone(),
                variant: None,
            },
        );
        self.recent_models.truncate(MAX_RECENT_MODELS);
        self.selected_model = Some(model);
        self.save_recent_models();
    }

    pub fn select_agent(&mut self, agent: String) {
        self.selected_agent = Some(agent);
    }

    fn save_recent_models(&self) {
        let Some(path) = self.recent_models_path.as_deref() else {
            return;
        };
        let file = RecentModelsFile {
            models: self
                .recent_models
                .iter()
                .map(|model| RecentModelEntry {
                    id: model.id.clone(),
                    provider_id: model.provider_id.clone(),
                })
                .collect(),
        };
        let Ok(content) = serde_json::to_string_pretty(&file) else {
            return;
        };
        if let Some(parent) = path.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            warn!(path = %parent.display(), %error, "failed to create recent model directory");
            return;
        }
        if let Err(error) = fs::write(path, content) {
            warn!(path = %path.display(), %error, "failed to persist recent models");
        }
    }
}

fn load_recent_models(path: Option<&Path>) -> Vec<ModelRef> {
    let Some(path) = path else {
        return Vec::new();
    };
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(file) = serde_json::from_str::<RecentModelsFile>(&content) else {
        warn!(path = %path.display(), "failed to parse persisted recent models");
        return Vec::new();
    };

    let mut models = Vec::with_capacity(MAX_RECENT_MODELS.min(file.models.len()));
    for entry in file.models {
        if entry.id.is_empty()
            || entry.provider_id.is_empty()
            || models.iter().any(|model: &ModelRef| {
                model.id == entry.id && model.provider_id == entry.provider_id
            })
        {
            continue;
        }
        models.push(ModelRef {
            id: entry.id,
            provider_id: entry.provider_id,
            variant: None,
        });
        if models.len() == MAX_RECENT_MODELS {
            break;
        }
    }
    models
}

#[cfg(test)]
mod tests {
    use super::{CatalogState, MAX_RECENT_MODELS};
    use crate::model::{ModelRef, ProviderCatalog, ProviderInfo};
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn replacing_provider_catalog_keeps_models_and_defaults_together() {
        let mut state = CatalogState::default();
        state.replace_providers(ProviderCatalog {
            providers: vec![ProviderInfo {
                id: "provider_1".to_owned(),
                ..ProviderInfo::default()
            }],
            default: HashMap::from([("anthropic".to_owned(), "claude".to_owned())]),
        });
        state.select_model(ModelRef {
            id: "model_1".to_owned(),
            provider_id: "provider_1".to_owned(),
            ..ModelRef::default()
        });

        assert_eq!(state.providers[0].id, "provider_1");
        assert_eq!(
            state.provider_defaults.get("anthropic"),
            Some(&"claude".to_owned())
        );
        assert_eq!(
            state.selected_model.as_ref().map(|model| model.id.as_str()),
            Some("model_1")
        );
    }

    #[test]
    fn selecting_models_keeps_the_ten_most_recent_without_duplicates() {
        let mut state = CatalogState::default();
        for index in 0..12 {
            state.select_model(ModelRef {
                id: format!("model_{index}"),
                provider_id: "provider".to_owned(),
                ..ModelRef::default()
            });
        }
        state.select_model(ModelRef {
            id: "model_5".to_owned(),
            provider_id: "provider".to_owned(),
            variant: Some("high".to_owned()),
        });

        assert_eq!(state.recent_models.len(), MAX_RECENT_MODELS);
        assert_eq!(state.recent_models[0].id, "model_5");
        assert!(state.recent_models[0].variant.is_none());
        assert_eq!(
            state
                .recent_models
                .iter()
                .filter(|model| model.id == "model_5")
                .count(),
            1
        );
        assert_eq!(
            state.selected_model.as_ref().unwrap().variant.as_deref(),
            Some("high")
        );
    }

    fn test_path(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "opencode-tui-rust-{name}-{}-{unique}.json",
            std::process::id(),
        ))
    }

    #[test]
    fn recent_models_survive_a_new_catalog_state() {
        let path = test_path("recent-models");
        let _ = std::fs::remove_file(&path);

        let mut first = CatalogState::with_recent_models_path(Some(path.clone()));
        first.select_model(ModelRef {
            provider_id: "provider-a".to_owned(),
            id: "model-a".to_owned(),
            ..ModelRef::default()
        });
        first.select_model(ModelRef {
            provider_id: "provider-b".to_owned(),
            id: "model-b".to_owned(),
            ..ModelRef::default()
        });

        let restored = CatalogState::with_recent_models_path(Some(path.clone()));
        assert_eq!(
            restored
                .recent_models
                .iter()
                .map(|model| (model.provider_id.as_str(), model.id.as_str()))
                .collect::<Vec<_>>(),
            vec![("provider-b", "model-b"), ("provider-a", "model-a")]
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn malformed_recent_models_file_falls_back_to_empty() {
        let path = test_path("malformed-recent-models");
        std::fs::write(&path, "{not valid json").expect("test file should be writable");

        let state = CatalogState::with_recent_models_path(Some(path.clone()));
        assert!(state.recent_models.is_empty());
        let _ = std::fs::remove_file(path);
    }
}
