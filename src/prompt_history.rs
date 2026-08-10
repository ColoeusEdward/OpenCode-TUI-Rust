use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;

/// Manages prompt history with navigation and stash support.
#[derive(Debug, Clone)]
pub struct PromptHistory {
    /// Stored prompts (most recent first)
    history: VecDeque<String>,
    /// Maximum number of prompts to keep
    max_size: usize,
    /// Current position in history (None = not navigating)
    position: Option<usize>,
    /// Stashed prompt (saved when navigating history)
    stash: Option<String>,
    /// Path to persistence file
    file_path: Option<PathBuf>,
}

impl Default for PromptHistory {
    fn default() -> Self {
        Self::new(100, None)
    }
}

impl PromptHistory {
    /// Create a new history with the given maximum size and optional persistence file.
    pub fn new(max_size: usize, file_path: Option<PathBuf>) -> Self {
        let mut history = Self {
            history: VecDeque::new(),
            max_size: max_size.max(1),
            position: None,
            stash: None,
            file_path,
        };
        history.load_from_disk();
        history
    }

    /// Get the default history file path.
    pub fn default_file_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("", "", "opencode-tui-rust").map(|dirs| {
            let data_dir = dirs.data_dir();
            let _ = fs::create_dir_all(data_dir);
            data_dir.join("prompt_history.txt")
        })
    }

    /// Load history from disk if a file path is configured.
    fn load_from_disk(&mut self) {
        let Some(path) = &self.file_path else {
            return;
        };

        if let Ok(content) = fs::read_to_string(path) {
            for line in content.lines().rev().take(self.max_size) {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    self.history.push_back(trimmed.to_owned());
                }
            }
        }
    }

    /// Save history to disk if a file path is configured.
    fn save_to_disk(&self) {
        let Some(path) = &self.file_path else {
            return;
        };

        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let content = self
            .history
            .iter()
            .rev()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        let _ = fs::write(path, content);
    }

    /// Add a prompt to history. Ignores empty or duplicate consecutive prompts.
    pub fn add(&mut self, prompt: &str) {
        let trimmed = prompt.trim();
        if trimmed.is_empty() {
            return;
        }

        // Don't add if it's the same as the most recent entry
        if self.history.front().is_some_and(|last| last == trimmed) {
            return;
        }

        self.history.push_front(trimmed.to_owned());

        // Trim to max size
        while self.history.len() > self.max_size {
            self.history.pop_back();
        }

        // Reset navigation state
        self.position = None;
        self.stash = None;

        // Persist to disk
        self.save_to_disk();
    }

    /// Navigate to the previous (older) prompt in history.
    /// Returns the prompt if available, or None if at the end.
    pub fn previous(&mut self, current: &str) -> Option<String> {
        if self.history.is_empty() {
            return None;
        }

        let new_position = match self.position {
            None => {
                // Start navigating - stash current prompt
                self.stash = Some(current.to_owned());
                0
            }
            Some(pos) => {
                let next = pos + 1;
                if next >= self.history.len() {
                    return None; // Already at oldest
                }
                next
            }
        };

        self.position = Some(new_position);
        self.history.get(new_position).cloned()
    }

    /// Navigate to the next (newer) prompt in history.
    /// Returns the prompt if available, or the stashed prompt if back at start.
    pub fn next(&mut self) -> Option<String> {
        let current_pos = self.position?;

        if current_pos == 0 {
            // Back to the stashed prompt
            self.position = None;
            return self.stash.take();
        }

        let new_position = current_pos - 1;
        self.position = Some(new_position);
        self.history.get(new_position).cloned()
    }

    /// Reset navigation state, clearing position and stash.
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.position = None;
        self.stash = None;
    }

    /// Get the number of prompts in history.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.history.len()
    }

    /// Check if history is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.history.is_empty()
    }

    /// Check if currently navigating history.
    pub fn is_navigating(&self) -> bool {
        self.position.is_some()
    }

    /// Get all history entries (most recent first).
    #[cfg(test)]
    pub fn entries(&self) -> Vec<&str> {
        self.history.iter().map(String::as_str).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::PromptHistory;

    #[test]
    fn adds_prompts_to_history() {
        let mut history = PromptHistory::new(10, None);

        history.add("first");
        history.add("second");
        history.add("third");

        assert_eq!(history.len(), 3);
        assert_eq!(history.entries(), vec!["third", "second", "first"]);
    }

    #[test]
    fn ignores_empty_and_duplicate_consecutive_prompts() {
        let mut history = PromptHistory::new(10, None);

        history.add("hello");
        history.add("");
        history.add("   ");
        history.add("hello");
        history.add("world");

        assert_eq!(history.len(), 2);
        assert_eq!(history.entries(), vec!["world", "hello"]);
    }

    #[test]
    fn trims_to_max_size() {
        let mut history = PromptHistory::new(3, None);

        history.add("1");
        history.add("2");
        history.add("3");
        history.add("4");

        assert_eq!(history.len(), 3);
        assert_eq!(history.entries(), vec!["4", "3", "2"]);
    }

    #[test]
    fn navigates_backward_through_history() {
        let mut history = PromptHistory::new(10, None);
        history.add("first");
        history.add("second");
        history.add("third");

        assert_eq!(history.previous("current"), Some("third".to_owned()));
        assert_eq!(history.previous("current"), Some("second".to_owned()));
        assert_eq!(history.previous("current"), Some("first".to_owned()));
        assert_eq!(history.previous("current"), None); // At oldest
    }

    #[test]
    fn navigates_forward_through_history() {
        let mut history = PromptHistory::new(10, None);
        history.add("first");
        history.add("second");

        history.previous("current");
        history.previous("current");

        assert_eq!(history.next(), Some("second".to_owned()));
        assert_eq!(history.next(), Some("current".to_owned())); // Back to stashed
        assert_eq!(history.next(), None); // Already at newest
    }

    #[test]
    fn stashes_and_restores_current_prompt() {
        let mut history = PromptHistory::new(10, None);
        history.add("old");

        assert_eq!(history.previous("typing..."), Some("old".to_owned()));
        assert_eq!(history.next(), Some("typing...".to_owned()));
    }

    #[test]
    fn reset_clears_navigation_state() {
        let mut history = PromptHistory::new(10, None);
        history.add("old");

        history.previous("current");
        assert!(history.is_navigating());

        history.reset();
        assert!(!history.is_navigating());
    }

    #[test]
    fn adding_new_prompt_resets_navigation() {
        let mut history = PromptHistory::new(10, None);
        history.add("first");
        history.add("second");

        history.previous("current");
        assert!(history.is_navigating());

        history.add("new");
        assert!(!history.is_navigating());
    }

    #[test]
    fn persists_and_loads_history() {
        use std::fs;

        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("opencode_tui_test_history.txt");

        // Clean up any existing test file
        let _ = fs::remove_file(&test_file);

        // Create history, add entries, and let it save
        {
            let mut history = PromptHistory::new(100, Some(test_file.clone()));
            history.add("first command");
            history.add("second command");
            history.add("third command");
            assert_eq!(history.len(), 3);
        }

        // Load from the same file in a new instance
        {
            let history = PromptHistory::new(100, Some(test_file.clone()));
            assert_eq!(history.len(), 3);
            assert_eq!(
                history.entries(),
                vec!["third command", "second command", "first command"]
            );
        }

        // Clean up
        let _ = fs::remove_file(&test_file);
    }
}
