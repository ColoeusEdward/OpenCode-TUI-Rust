use crate::model::CommandInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommand {
    Model,
    Skill,
    Agent,
    Variant,
    Timeline,
    Fork,
    Share,
    Unshare,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlashCommandInfo {
    pub command: SlashCommand,
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
}

#[derive(Debug, Clone)]
pub enum CommandOption {
    BuiltIn(SlashCommandInfo),
    Server(CommandInfo),
}

impl CommandOption {
    pub fn name(&self) -> &str {
        match self {
            Self::BuiltIn(command) => command.name,
            Self::Server(command) => &command.name,
        }
    }

    pub fn description(&self) -> String {
        match self {
            Self::BuiltIn(command) => command.description.to_owned(),
            Self::Server(command) => {
                let mut details = command
                    .description
                    .clone()
                    .or_else(|| {
                        command
                            .template
                            .lines()
                            .find(|line| !line.trim().is_empty())
                            .map(str::to_owned)
                    })
                    .unwrap_or_else(|| "Server command".to_owned());
                if let Some(agent) = command.agent.as_deref() {
                    details.push_str(&format!("  agent:{agent}"));
                }
                if let Some(model) = command.model.as_ref() {
                    details.push_str(&format!("  model:{}/{}", model.provider_id, model.id));
                }
                if command.subtask {
                    details.push_str("  subtask");
                }
                details
            }
        }
    }
}

const COMMANDS: [SlashCommandInfo; 8] = [
    SlashCommandInfo {
        command: SlashCommand::Model,
        name: "model",
        aliases: &["models", "mo"],
        description: "Select the model for the next prompt",
    },
    SlashCommandInfo {
        command: SlashCommand::Skill,
        name: "skill",
        aliases: &["skills"],
        description: "Insert a skill command into the prompt",
    },
    SlashCommandInfo {
        command: SlashCommand::Agent,
        name: "agent",
        aliases: &["agents"],
        description: "Select the agent for the next prompt",
    },
    SlashCommandInfo {
        command: SlashCommand::Variant,
        name: "variant",
        aliases: &["variants", "va"],
        description: "Select the variant for the next prompt",
    },
    SlashCommandInfo {
        command: SlashCommand::Timeline,
        name: "timeline",
        aliases: &[],
        description: "Jump to a user prompt in the current session",
    },
    SlashCommandInfo {
        command: SlashCommand::Fork,
        name: "fork",
        aliases: &[],
        description: "Fork the current session",
    },
    SlashCommandInfo {
        command: SlashCommand::Share,
        name: "share",
        aliases: &[],
        description: "Create a shareable link for the current session",
    },
    SlashCommandInfo {
        command: SlashCommand::Unshare,
        name: "unshare",
        aliases: &[],
        description: "Remove the shareable link from the current session",
    },
];

pub fn commands() -> &'static [SlashCommandInfo] {
    &COMMANDS
}

pub fn slash_query(text: &str) -> Option<&str> {
    let query = text.strip_prefix('/')?;
    if query.chars().any(char::is_whitespace) {
        return None;
    }
    Some(query)
}

pub fn matching_commands(query: &str) -> Vec<SlashCommandInfo> {
    let query = query.to_ascii_lowercase();
    commands()
        .iter()
        .copied()
        .filter(|command| {
            command.name.starts_with(&query)
                || command
                    .aliases
                    .iter()
                    .any(|alias| alias.starts_with(&query))
        })
        .collect()
}

pub fn matching_commands_with_server(
    query: &str,
    server_commands: &[CommandInfo],
) -> Vec<CommandOption> {
    let query = query.to_ascii_lowercase();
    let mut options = matching_commands(&query)
        .into_iter()
        .map(CommandOption::BuiltIn)
        .collect::<Vec<_>>();
    options.extend(
        server_commands
            .iter()
            .filter(|command| {
                if command.name.is_empty() {
                    return false;
                }
                query.is_empty()
                    || command.name.to_ascii_lowercase().starts_with(&query)
                    || command.description.as_deref().is_some_and(|description| {
                        description.to_ascii_lowercase().contains(&query)
                    })
            })
            .cloned()
            .map(CommandOption::Server),
    );
    options.sort_by_key(|option| option.name().to_ascii_lowercase());
    options
}

#[cfg(test)]
mod tests {
    use super::{
        CommandOption, SlashCommand, matching_commands, matching_commands_with_server, slash_query,
    };
    use crate::model::CommandInfo;

    #[test]
    fn slash_query_only_matches_a_single_token_at_prompt_start() {
        assert_eq!(slash_query("/model"), Some("model"));
        assert_eq!(slash_query("/"), Some(""));
        assert_eq!(slash_query("hello /model"), None);
        assert_eq!(slash_query("/model extra"), None);
        assert_eq!(slash_query("/model\nextra"), None);
    }

    #[test]
    fn command_matching_supports_aliases_and_case_insensitive_prefixes() {
        assert_eq!(matching_commands("mo")[0].command, SlashCommand::Model);
        assert_eq!(matching_commands("SKI")[0].command, SlashCommand::Skill);
        assert_eq!(matching_commands("agents")[0].command, SlashCommand::Agent);
        assert_eq!(matching_commands("va")[0].command, SlashCommand::Variant);
        assert_eq!(
            matching_commands("timeline")[0].command,
            SlashCommand::Timeline
        );
        assert_eq!(matching_commands("fork")[0].command, SlashCommand::Fork);
        assert_eq!(matching_commands("share")[0].command, SlashCommand::Share);
        assert_eq!(
            matching_commands("unshare")[0].command,
            SlashCommand::Unshare
        );
        assert!(matching_commands("missing").is_empty());
    }

    #[test]
    fn command_matching_merges_server_commands_and_searches_descriptions() {
        let options = matching_commands_with_server(
            "review",
            &[CommandInfo {
                name: "review".to_owned(),
                description: Some("Review the current diff".to_owned()),
                ..CommandInfo::default()
            }],
        );

        assert!(
            matches!(options.as_slice(), [CommandOption::Server(command)] if command.name == "review")
        );
        assert_eq!(options[0].description(), "Review the current diff");
    }
}
