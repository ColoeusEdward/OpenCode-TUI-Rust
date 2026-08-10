use crate::model::{AgentInfo, ReferenceInfo, WorkspaceFile};

const MAX_MENTION_OPTIONS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MentionKind {
    File,
    Reference,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionOption {
    pub name: String,
    pub description: String,
    pub kind: MentionKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionContext {
    pub query: String,
    pub start: usize,
    pub end: usize,
}

pub fn mention_context(text: &str, cursor: usize) -> Option<MentionContext> {
    let chars = text.chars().collect::<Vec<_>>();
    let cursor = cursor.min(chars.len());
    let line_start = chars[..cursor]
        .iter()
        .rposition(|character| *character == '\n')
        .map_or(0, |index| index + 1);
    let token_start = chars[line_start..cursor]
        .iter()
        .rposition(|character| character.is_whitespace())
        .map_or(line_start, |index| line_start + index + 1);

    if chars.get(token_start) != Some(&'@')
        || (token_start > 0 && !is_mention_boundary(chars[token_start - 1]))
    {
        return None;
    }

    let end = chars[cursor..]
        .iter()
        .position(|character| character.is_whitespace())
        .map_or(chars.len(), |index| cursor + index);
    let query = chars[token_start + 1..cursor].iter().collect::<String>();
    Some(MentionContext {
        query,
        start: token_start,
        end,
    })
}

pub fn mention_options(
    files: &[WorkspaceFile],
    references: &[ReferenceInfo],
    agents: &[AgentInfo],
    query: &str,
) -> Vec<MentionOption> {
    let mut options = files
        .iter()
        .filter_map(|file| {
            fuzzy_score(query, &file.path).map(|score| {
                (
                    score,
                    MentionOption {
                        name: file.path.clone(),
                        description: if file.is_directory {
                            "directory".to_owned()
                        } else {
                            "file".to_owned()
                        },
                        kind: MentionKind::File,
                    },
                )
            })
        })
        .chain(references.iter().filter_map(|reference| {
            if reference.hidden || reference.name.is_empty() {
                return None;
            }
            let score = fuzzy_score(query, &reference.name).or_else(|| {
                fuzzy_score(
                    query,
                    reference.description.as_deref().unwrap_or(&reference.path),
                )
                .map(|score| score.saturating_sub(500))
            })?;
            Some((
                score,
                MentionOption {
                    name: reference.name.clone(),
                    description: reference
                        .description
                        .as_deref()
                        .map(compact_description)
                        .filter(|description| !description.is_empty())
                        .unwrap_or_else(|| reference.path.clone()),
                    kind: MentionKind::Reference,
                },
            ))
        }))
        .chain(agents.iter().filter_map(|agent| {
            if !agent.is_mentionable() {
                return None;
            }
            let score = fuzzy_score(query, &agent.name).or_else(|| {
                fuzzy_score(query, &agent.description).map(|score| score.saturating_sub(1_000))
            })?;
            Some((
                score,
                MentionOption {
                    name: agent.name.clone(),
                    description: if agent.description.is_empty() {
                        "agent".to_owned()
                    } else {
                        compact_description(&agent.description)
                    },
                    kind: MentionKind::Agent,
                },
            ))
        }))
        .collect::<Vec<_>>();

    options.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.kind.cmp(&right.1.kind))
            .then_with(|| left.1.name.cmp(&right.1.name))
    });
    options
        .into_iter()
        .take(MAX_MENTION_OPTIONS)
        .map(|(_, option)| option)
        .collect()
}

fn fuzzy_score(query: &str, candidate: &str) -> Option<i32> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Some(0);
    }
    let candidate = candidate.to_lowercase();
    if candidate == query {
        return Some(10_000);
    }
    if candidate.starts_with(&query) {
        return Some(8_000 - candidate.chars().count() as i32);
    }
    if let Some(index) = candidate.find(&query) {
        return Some(6_000 - index as i32);
    }

    let candidate_chars = candidate.chars().collect::<Vec<_>>();
    let mut cursor = 0usize;
    let mut gaps = 0usize;
    for character in query.chars() {
        let relative = candidate_chars[cursor..]
            .iter()
            .position(|candidate| *candidate == character)?;
        gaps += relative;
        cursor += relative + 1;
    }
    Some(3_000 - gaps as i32 - cursor as i32)
}

fn is_mention_boundary(character: char) -> bool {
    character.is_whitespace() || "([{\"'".contains(character)
}

fn compact_description(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Ord for MentionKind {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (*self as u8).cmp(&(*other as u8))
    }
}

impl PartialOrd for MentionKind {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_MENTION_OPTIONS, MentionKind, mention_context, mention_options};
    use crate::model::{AgentInfo, ReferenceInfo, WorkspaceFile};

    #[test]
    fn finds_the_current_at_token_using_character_offsets() {
        let context = mention_context("检查 @你好/README.md now", 16).expect("mention context");

        assert_eq!(context.query, "你好/README.md");
        assert_eq!(context.start, 3);
        assert_eq!(context.end, 16);
    }

    #[test]
    fn ignores_embedded_at_signs() {
        assert!(mention_context("email@example.com", 17).is_none());
        assert!(mention_context("hello /@agent", 13).is_none());
    }

    #[test]
    fn combines_files_and_mentionable_agents_in_stable_order() {
        let options = mention_options(
            &[WorkspaceFile {
                path: "src/main.rs".to_owned(),
                is_directory: false,
            }],
            &[],
            &[
                AgentInfo {
                    name: "explore".to_owned(),
                    mode: "subagent".to_owned(),
                    description: "Find files".to_owned(),
                    ..AgentInfo::default()
                },
                AgentInfo {
                    name: "build".to_owned(),
                    mode: "primary".to_owned(),
                    ..AgentInfo::default()
                },
            ],
            "",
        );

        assert_eq!(options.len(), 2);
        assert_eq!(options[0].kind, MentionKind::File);
        assert_eq!(options[1].name, "explore");
    }

    #[test]
    fn fuzzy_matching_ranks_relevant_files_and_caps_the_result_set() {
        let files = (0..12)
            .map(|index| WorkspaceFile {
                path: if index == 0 {
                    "src/main.rs".to_owned()
                } else {
                    format!("src/module{index}.rs")
                },
                is_directory: false,
            })
            .collect::<Vec<_>>();

        let options = mention_options(&files, &[], &[], "smr");

        assert_eq!(options.len(), MAX_MENTION_OPTIONS);
        assert_eq!(options[0].name, "src/main.rs");
    }

    #[test]
    fn agent_description_is_a_lower_priority_fuzzy_match() {
        let agents = vec![
            AgentInfo {
                name: "explore".to_owned(),
                mode: "subagent".to_owned(),
                description: "Inspect files".to_owned(),
                ..AgentInfo::default()
            },
            AgentInfo {
                name: "inspect".to_owned(),
                mode: "subagent".to_owned(),
                description: "Build the project".to_owned(),
                ..AgentInfo::default()
            },
        ];

        let options = mention_options(&[], &[], &agents, "inspect");

        assert_eq!(options[0].name, "inspect");
        assert_eq!(options[1].name, "explore");
        assert_eq!(options[1].kind, MentionKind::Agent);
    }

    #[test]
    fn reference_aliases_are_searchable_and_visible() {
        let options = mention_options(
            &[],
            &[ReferenceInfo {
                name: "docs".to_owned(),
                path: "C:/workspace/docs".to_owned(),
                description: Some("Project documentation".to_owned()),
                ..ReferenceInfo::default()
            }],
            &[],
            "doc",
        );

        assert_eq!(options.len(), 1);
        assert_eq!(options[0].kind, MentionKind::Reference);
        assert_eq!(options[0].description, "Project documentation");
    }
}
