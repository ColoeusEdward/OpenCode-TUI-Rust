use crate::model::CommandInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommand {
    Build,
    Plan,
    Model,
    Skill,
    Agent,
    Variant,
    Compact,
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

const COMMANDS: [SlashCommandInfo; 11] = [
    SlashCommandInfo {
        command: SlashCommand::Build,
        name: "build",
        aliases: &[],
        description: "Switch to build mode",
    },
    SlashCommandInfo {
        command: SlashCommand::Plan,
        name: "plan",
        aliases: &[],
        description: "Switch to plan mode",
    },
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
        command: SlashCommand::Compact,
        name: "compact",
        aliases: &["summarize"],
        description: "Summarize and compact the current session context",
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
    let mut matches = commands()
        .iter()
        .copied()
        .filter_map(|command| built_in_match_score(&command, &query).map(|score| (score, command)))
        .collect::<Vec<_>>();
    matches.sort_by(|(left_score, left), (right_score, right)| {
        left_score
            .cmp(right_score)
            .then_with(|| left.name.cmp(right.name))
    });
    matches.into_iter().map(|(_, command)| command).collect()
}

pub fn matching_commands_with_server(
    query: &str,
    server_commands: &[CommandInfo],
) -> Vec<CommandOption> {
    let query = query.to_ascii_lowercase();
    let mut options = matching_commands(&query)
        .into_iter()
        .filter_map(|command| {
            built_in_match_score(&command, &query)
                .map(|score| (score, CommandOption::BuiltIn(command)))
        })
        .collect::<Vec<_>>();
    options.extend(server_commands.iter().filter_map(|command| {
        if command.name.is_empty() {
            return None;
        }
        let name_score = text_match_score(&command.name, &query);
        let description_score = command
            .description
            .as_deref()
            .and_then(|description| text_match_score(description, &query))
            .map(|score| 10_000 + score);
        name_score
            .into_iter()
            .chain(description_score)
            .min()
            .map(|score| (score, CommandOption::Server(command.clone())))
    }));
    options.sort_by(|(left_score, left), (right_score, right)| {
        left_score
            .cmp(right_score)
            .then_with(|| {
                left.name()
                    .to_ascii_lowercase()
                    .cmp(&right.name().to_ascii_lowercase())
            })
            .then_with(|| option_kind_rank(left).cmp(&option_kind_rank(right)))
    });
    options.into_iter().map(|(_, option)| option).collect()
}

fn built_in_match_score(command: &SlashCommandInfo, query: &str) -> Option<usize> {
    let name_score = text_match_score(command.name, query);
    let alias_score = command
        .aliases
        .iter()
        .filter_map(|alias| text_match_score(alias, query).map(|score| score + 25))
        .min();
    name_score.into_iter().chain(alias_score).min()
}

fn option_kind_rank(option: &CommandOption) -> u8 {
    match option {
        CommandOption::BuiltIn(_) => 0,
        CommandOption::Server(_) => 1,
    }
}

/// Case-insensitive fuzzy score used by both command discovery surfaces.
/// Lower scores represent stronger matches.
pub(crate) fn text_match_score(candidate: &str, query: &str) -> Option<usize> {
    let candidate = candidate.to_ascii_lowercase();
    let query = query.to_ascii_lowercase();
    if query.is_empty() {
        return Some(0);
    }
    if candidate == query {
        return Some(0);
    }
    if candidate.starts_with(&query) {
        return Some(100 + candidate.len().saturating_sub(query.len()));
    }
    if let Some(position) = candidate.find(&query) {
        return Some(1_000 + position * 10 + candidate.len().saturating_sub(query.len()));
    }

    let mut candidate_indices = candidate.char_indices();
    let mut first = None;
    let mut previous = None;
    let mut gaps = 0;
    for query_character in query.chars() {
        let (index, _) = candidate_indices.find(|(_, character)| *character == query_character)?;
        first.get_or_insert(index);
        if let Some(previous) = previous {
            gaps += index.saturating_sub(previous + 1);
        }
        previous = Some(index);
    }
    Some(
        2_000
            + first.unwrap_or_default() * 10
            + gaps * 5
            + candidate.len().saturating_sub(query.len()),
    )
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
        assert_eq!(matching_commands("build")[0].command, SlashCommand::Build);
        assert_eq!(matching_commands("plan")[0].command, SlashCommand::Plan);
        assert_eq!(matching_commands("mo")[0].command, SlashCommand::Model);
        assert_eq!(matching_commands("SKI")[0].command, SlashCommand::Skill);
        assert_eq!(matching_commands("agents")[0].command, SlashCommand::Agent);
        assert_eq!(matching_commands("va")[0].command, SlashCommand::Variant);
        assert_eq!(
            matching_commands("summarize")[0].command,
            SlashCommand::Compact
        );
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

    #[test]
    fn stronger_command_matches_sort_before_weaker_matches() {
        let options = matching_commands_with_server(
            "share",
            &[
                CommandInfo {
                    name: "share-report".to_owned(),
                    ..CommandInfo::default()
                },
                CommandInfo {
                    name: "report".to_owned(),
                    description: Some("Share a report".to_owned()),
                    ..CommandInfo::default()
                },
            ],
        );

        assert_eq!(
            options.iter().map(CommandOption::name).collect::<Vec<_>>(),
            vec!["share", "share-report", "unshare", "report"]
        );
    }
}
