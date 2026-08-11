use crate::model::{
    AgentInfo, CommandInfo, ModelRef, ProviderCatalog, ProviderInfo, ReferenceInfo, Skill,
    WorkspaceFile,
};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
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
}

impl CatalogState {
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
        self.recent_models.truncate(10);
        self.selected_model = Some(model);
    }

    pub fn select_agent(&mut self, agent: String) {
        self.selected_agent = Some(agent);
    }
}

#[cfg(test)]
mod tests {
    use super::CatalogState;
    use crate::model::{ModelRef, ProviderCatalog, ProviderInfo};
    use std::collections::HashMap;

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

        assert_eq!(state.recent_models.len(), 10);
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
}
