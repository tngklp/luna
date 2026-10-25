//! The slash-command palette.
//!
//! Typing `/` opens a filtered list of commands. It is a plain data structure so
//! the filtering and selection rules can be tested without a terminal.

/// One command the composer can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    /// What the user types, including the leading slash.
    pub name: &'static str,
    /// One line, shown beside the name.
    pub description: &'static str,
    /// Placeholder for the argument, when the command takes one.
    pub argument: Option<&'static str>,
}

/// Every command the TUI understands.
pub const COMMANDS: &[Command] = &[
    Command { name: "/help", description: "Show the keys and commands", argument: None },
    Command { name: "/new", description: "Start a new session", argument: None },
    Command { name: "/model", description: "Switch model", argument: Some("<provider/model>") },
    Command {
        name: "/thinking",
        description: "Set the reasoning effort",
        argument: Some("<off|low|medium|high|max>"),
    },
    Command { name: "/sessions", description: "Browse saved sessions", argument: None },
    Command { name: "/compact", description: "Summarise the conversation", argument: None },
    Command { name: "/copy", description: "Copy the transcript", argument: None },
    Command { name: "/clear", description: "Clear the transcript", argument: None },
    Command { name: "/quit", description: "Exit Luna", argument: None },
];

/// The palette's state: what has been typed and which row is highlighted.
#[derive(Debug, Clone, Default)]
pub struct Palette {
    /// The text after the slash, used to filter.
    query: String,
    /// Index into [`Palette::matches`].
    selected: usize,
}

impl Palette {
    /// Rebuild the palette from the composer's contents.
    ///
    /// Returns `None` when the composer is not a command, which closes the
    /// palette. A command with an argument (`/model gpt`) also closes it: the
    /// user has moved on to typing the argument.
    pub fn from_input(input: &str) -> Option<Palette> {
        let rest = input.strip_prefix('/')?;
        if rest.contains(' ') {
            return None;
        }
        Some(Palette { query: rest.to_string(), selected: 0 })
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// Whether nothing matches, so the caller can say so instead of showing an
    /// empty box.
    pub fn is_empty(&self) -> bool {
        self.matches().is_empty()
    }

    /// Commands matching the query, best first.
    pub fn matches(&self) -> Vec<&'static Command> {
        let query = self.query.to_ascii_lowercase();
        let mut scored: Vec<(u8, &'static Command)> = COMMANDS
            .iter()
            .filter_map(|command| {
                let name = command.name.trim_start_matches('/').to_ascii_lowercase();
                if query.is_empty() {
                    return Some((1, command));
                }
                if name.starts_with(&query) {
                    // A prefix match is what the user is most likely after.
                    return Some((0, command));
                }
                if name.contains(&query)
                    || command.description.to_ascii_lowercase().contains(&query)
                {
                    return Some((2, command));
                }
                None
            })
            .collect();
        scored.sort_by_key(|(rank, command)| (*rank, command.name));
        scored.into_iter().map(|(_, command)| command).collect()
    }

    pub fn selected(&self) -> usize {
        self.selected.min(self.matches().len().saturating_sub(1))
    }

    pub fn select_next(&mut self) {
        let count = self.matches().len();
        if count > 0 {
            self.selected = (self.selected() + 1) % count;
        }
    }

    pub fn select_previous(&mut self) {
        let count = self.matches().len();
        if count > 0 {
            self.selected = (self.selected() + count - 1) % count;
        }
    }

    /// The highlighted command, if any.
    pub fn current(&self) -> Option<&'static Command> {
        self.matches().get(self.selected()).copied()
    }

    /// The text to put in the composer when the highlighted command is accepted.
    pub fn completion(&self) -> Option<String> {
        let command = self.current()?;
        Some(match command.argument {
            // A command that takes an argument is completed with a trailing
            // space so the user can type it straight away.
            Some(_) => format!("{} ", command.name),
            None => command.name.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_slash_lists_everything() {
        let palette = Palette::from_input("/").unwrap();
        assert_eq!(palette.matches().len(), COMMANDS.len());
    }

    #[test]
    fn a_prefix_filters_and_ranks_first() {
        let palette = Palette::from_input("/mo").unwrap();
        let names: Vec<&str> = palette.matches().iter().map(|c| c.name).collect();
        assert_eq!(names.first(), Some(&"/model"));
    }

    #[test]
    fn matching_is_case_insensitive() {
        let palette = Palette::from_input("/HE").unwrap();
        assert_eq!(palette.matches()[0].name, "/help");
    }

    #[test]
    fn a_description_match_is_offered_too() {
        let palette = Palette::from_input("/reasoning").unwrap();
        assert_eq!(palette.matches()[0].name, "/thinking");
    }

    #[test]
    fn an_argument_closes_the_palette() {
        assert!(Palette::from_input("/model gpt-5").is_none());
    }

    #[test]
    fn plain_text_is_not_a_command() {
        assert!(Palette::from_input("hello").is_none());
        assert!(Palette::from_input("").is_none());
    }

    #[test]
    fn selection_wraps_in_both_directions() {
        let mut palette = Palette::from_input("/").unwrap();
        let count = palette.matches().len();
        palette.select_previous();
        assert_eq!(palette.selected(), count - 1);
        palette.select_next();
        assert_eq!(palette.selected(), 0);
    }

    #[test]
    fn completing_an_argument_command_adds_a_space() {
        let palette = Palette::from_input("/mod").unwrap();
        assert_eq!(palette.completion().as_deref(), Some("/model "));
    }

    #[test]
    fn completing_a_plain_command_adds_nothing() {
        let palette = Palette::from_input("/hel").unwrap();
        assert_eq!(palette.completion().as_deref(), Some("/help"));
    }

    #[test]
    fn no_match_means_no_selection() {
        let palette = Palette::from_input("/zzzz").unwrap();
        assert!(palette.is_empty());
        assert!(palette.current().is_none());
        assert!(palette.completion().is_none());
    }

    #[test]
    fn every_command_has_a_description() {
        for command in COMMANDS {
            assert!(!command.description.is_empty(), "{} has no description", command.name);
            assert!(command.name.starts_with('/'), "{} is not a slash command", command.name);
        }
    }
}
