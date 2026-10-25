//! Extensibility: skills, custom slash commands, hooks and plugins.
//!
//! All four are files on disk, so a user can add capability without rebuilding
//! Luna. They are loaded from two roots, project first:
//!
//! * `./.luna/…` — belongs to the repository, shared with the team.
//! * `~/.luna/…` — belongs to the user, available everywhere.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{LunaError, Result};

/// A markdown file with YAML-ish frontmatter.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    /// Frontmatter fields, as strings.
    pub meta: BTreeMap<String, String>,
    /// Everything after the frontmatter.
    pub body: String,
    /// Where it came from.
    pub path: PathBuf,
}

impl Document {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.meta.get(key).map(|value| value.as_str())
    }

    /// The display name: `name` from the frontmatter, else the file stem.
    pub fn name(&self) -> String {
        self.get("name")
            .map(|value| value.to_string())
            .or_else(|| self.path.file_stem().map(|stem| stem.to_string_lossy().to_string()))
            .unwrap_or_else(|| "untitled".to_string())
    }

    pub fn description(&self) -> String {
        self.get("description").unwrap_or("").to_string()
    }
}

/// Split a markdown file into frontmatter and body.
///
/// The frontmatter is deliberately simple — `key: value` lines between `---`
/// fences — because a full YAML parser is a large dependency for four fields.
pub fn parse_document(path: &Path, text: &str) -> Document {
    let mut meta = BTreeMap::new();
    let mut body = text.to_string();

    let trimmed = text.strip_prefix('\u{feff}').unwrap_or(text);
    if let Some(rest) = trimmed.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            let front = &rest[..end];
            for line in front.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((key, value)) = line.split_once(':') {
                    let value = value.trim().trim_matches('"').trim_matches('\'');
                    meta.insert(key.trim().to_string(), value.to_string());
                }
            }
            body = rest[end + 4..].trim_start_matches('\n').to_string();
        }
    }

    Document { meta, body, path: path.to_path_buf() }
}

/// A skill: instructions the agent can pull in on demand.
#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub body: String,
    pub path: PathBuf,
}

impl Skill {
    pub fn from_document(document: Document) -> Skill {
        Skill {
            name: document.name(),
            description: document.description(),
            body: document.body,
            path: document.path,
        }
    }

    /// The line the system prompt shows, so the model knows the skill exists.
    pub fn summary(&self) -> String {
        if self.description.is_empty() {
            format!("- {}", self.name)
        } else {
            format!("- {}: {}", self.name, self.description)
        }
    }
}

/// A user-defined slash command.
#[derive(Debug, Clone)]
pub struct CustomCommand {
    /// What the user types, without the slash.
    pub name: String,
    pub description: String,
    /// The prompt template. `$ARGUMENTS` is replaced with what the user typed.
    pub template: String,
    pub path: PathBuf,
}

impl CustomCommand {
    pub fn from_document(document: Document) -> CustomCommand {
        CustomCommand {
            name: document.name(),
            description: document.description(),
            template: document.body,
            path: document.path,
        }
    }

    /// Fill in the template.
    pub fn render(&self, arguments: &str) -> String {
        if self.template.contains("$ARGUMENTS") {
            self.template.replace("$ARGUMENTS", arguments)
        } else if arguments.trim().is_empty() {
            self.template.clone()
        } else {
            format!("{}\n\n{}", self.template.trim_end(), arguments)
        }
    }
}

/// When a hook runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    SessionStart,
    SessionEnd,
    PreTool,
    PostTool,
    PrePrompt,
}

impl HookEvent {
    pub fn parse(value: &str) -> Option<HookEvent> {
        match value {
            "session_start" => Some(HookEvent::SessionStart),
            "session_end" => Some(HookEvent::SessionEnd),
            "pre_tool" => Some(HookEvent::PreTool),
            "post_tool" => Some(HookEvent::PostTool),
            "pre_prompt" => Some(HookEvent::PrePrompt),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            HookEvent::SessionStart => "session_start",
            HookEvent::SessionEnd => "session_end",
            HookEvent::PreTool => "pre_tool",
            HookEvent::PostTool => "post_tool",
            HookEvent::PrePrompt => "pre_prompt",
        }
    }
}

/// A shell command that runs on a lifecycle event.
#[derive(Debug, Clone)]
pub struct Hook {
    pub event: HookEvent,
    pub command: String,
    /// Only run for these tools, when the event is tool-related.
    pub tools: Vec<String>,
    pub path: PathBuf,
}

impl Hook {
    pub fn from_document(document: Document) -> Option<Hook> {
        let event = HookEvent::parse(document.get("event")?)?;
        let command = document.get("command")?.to_string();
        let tools = document
            .get("tools")
            .map(|value| {
                value
                    .split(',')
                    .map(|name| name.trim().to_string())
                    .filter(|name| !name.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        Some(Hook { event, command, tools, path: document.path })
    }

    /// Whether this hook applies to a tool call.
    pub fn matches_tool(&self, tool: &str) -> bool {
        self.tools.is_empty() || self.tools.iter().any(|name| name == tool)
    }
}

/// A plugin: a directory with a manifest and an optional command.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plugin {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: String,
    /// A shell command that receives JSON on stdin and returns tools on stdout.
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub commands: Vec<String>,
    /// Filled in after loading; not part of the manifest.
    #[serde(skip)]
    pub path: PathBuf,
}

/// Everything loaded from disk.
#[derive(Debug, Clone, Default)]
pub struct Extensions {
    pub skills: Vec<Skill>,
    pub commands: Vec<CustomCommand>,
    pub hooks: Vec<Hook>,
    pub plugins: Vec<Plugin>,
    /// Files that could not be read, with the reason.
    pub problems: Vec<String>,
}

impl Extensions {
    /// Load from a project root and a user root.
    ///
    /// The user root is read first so a project file of the same name replaces
    /// it: a repository's own skill should win over the user's.
    pub fn load(project: &Path, user: &Path) -> Extensions {
        let mut extensions = Extensions::default();
        for root in [user, project] {
            extensions.load_root(root);
        }
        extensions
    }

    fn load_root(&mut self, root: &Path) {
        self.load_skills(&root.join("skills"));
        self.load_commands(&root.join("commands"));
        self.load_hooks(&root.join("hooks"));
        self.load_plugins(&root.join("plugins"));
    }

    fn load_skills(&mut self, dir: &Path) {
        for (path, text) in read_markdown(dir) {
            let skill = Skill::from_document(parse_document(&path, &text));
            // A project skill shadows a user skill of the same name.
            self.skills.retain(|existing| existing.name != skill.name);
            self.skills.push(skill);
        }
    }

    fn load_commands(&mut self, dir: &Path) {
        for (path, text) in read_markdown(dir) {
            let command = CustomCommand::from_document(parse_document(&path, &text));
            self.commands.retain(|existing| existing.name != command.name);
            self.commands.push(command);
        }
    }

    fn load_hooks(&mut self, dir: &Path) {
        for (path, text) in read_markdown(dir) {
            match Hook::from_document(parse_document(&path, &text)) {
                Some(hook) => self.hooks.push(hook),
                None => self
                    .problems
                    .push(format!("{}: a hook needs `event` and `command`", path.display())),
            }
        }
    }

    fn load_plugins(&mut self, dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let manifest = entry.path().join("plugin.json");
            if !manifest.exists() {
                continue;
            }
            match std::fs::read_to_string(&manifest) {
                Ok(text) => match serde_json::from_str::<Plugin>(&text) {
                    Ok(mut plugin) => {
                        plugin.path = entry.path();
                        self.plugins.retain(|existing| existing.name != plugin.name);
                        self.plugins.push(plugin);
                    }
                    Err(err) => {
                        self.problems.push(format!("{}: {err}", manifest.display()));
                    }
                },
                Err(err) => self.problems.push(format!("{}: {err}", manifest.display())),
            }
        }
    }

    pub fn skill(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|skill| skill.name == name)
    }

    pub fn command(&self, name: &str) -> Option<&CustomCommand> {
        self.commands.iter().find(|command| command.name == name)
    }

    pub fn hooks_for(&self, event: HookEvent) -> impl Iterator<Item = &Hook> {
        self.hooks.iter().filter(move |hook| hook.event == event)
    }

    /// The block the system prompt shows so the model knows what is available.
    pub fn prompt_section(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }
        let mut out = String::from("Skills you can use:\n");
        for skill in &self.skills {
            out.push_str(&skill.summary());
            out.push('\n');
        }
        out
    }
}

/// Read every `.md` file in a directory, one level deep.
fn read_markdown(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // A skill may be a directory with a SKILL.md inside.
            let nested = path.join("SKILL.md");
            if nested.exists() {
                if let Ok(text) = std::fs::read_to_string(&nested) {
                    out.push((nested, text));
                }
            }
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("md") {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&path) {
            out.push((path, text));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Run a hook, returning its output.
///
/// A failing hook is reported but never stops the agent: a broken hook should
/// not make Luna unusable.
pub async fn run_hook(hook: &Hook, cwd: &Path, payload: &serde_json::Value) -> Result<String> {
    use std::process::Stdio;

    let child = tokio::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" })
        .args(if cfg!(windows) {
            vec!["/C".to_string(), hook.command.clone()]
        } else {
            vec!["-c".to_string(), hook.command.clone()]
        })
        .current_dir(cwd)
        .env("LUNA_EVENT", hook.event.as_str())
        .env("LUNA_PAYLOAD", payload.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|err| LunaError::tool(format!("could not run the hook: {err}")))?;

    let stdout = String::from_utf8_lossy(&child.stdout).trim_end().to_string();
    if child.status.success() {
        Ok(stdout)
    } else {
        let stderr = String::from_utf8_lossy(&child.stderr).trim_end().to_string();
        Err(LunaError::tool(format!("hook failed: {stderr}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("luna-ext-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn frontmatter_is_parsed() {
        let doc = parse_document(
            Path::new("/x/review.md"),
            "---\nname: review\ndescription: Reviews code\n---\n\nDo the review.\n",
        );
        assert_eq!(doc.name(), "review");
        assert_eq!(doc.description(), "Reviews code");
        assert_eq!(doc.body.trim(), "Do the review.");
    }

    #[test]
    fn a_file_without_frontmatter_is_all_body() {
        let doc = parse_document(Path::new("/x/plain.md"), "just text");
        assert_eq!(doc.body, "just text");
        assert_eq!(doc.name(), "plain");
    }

    #[test]
    fn quoted_values_are_unquoted() {
        let doc = parse_document(Path::new("/x/a.md"), "---\nname: \"quoted\"\n---\nbody");
        assert_eq!(doc.name(), "quoted");
    }

    #[test]
    fn a_command_template_substitutes_arguments() {
        let command = CustomCommand {
            name: "fix".into(),
            description: String::new(),
            template: "Fix this: $ARGUMENTS".into(),
            path: PathBuf::new(),
        };
        assert_eq!(command.render("the parser"), "Fix this: the parser");
    }

    #[test]
    fn a_template_without_a_placeholder_appends_arguments() {
        let command = CustomCommand {
            name: "fix".into(),
            description: String::new(),
            template: "Fix the bug.".into(),
            path: PathBuf::new(),
        };
        assert_eq!(command.render("in main.rs"), "Fix the bug.\n\nin main.rs");
        assert_eq!(command.render(""), "Fix the bug.");
    }

    #[test]
    fn skills_and_commands_load_from_disk() {
        let root = temp_dir("load");
        std::fs::create_dir_all(root.join("skills")).unwrap();
        std::fs::create_dir_all(root.join("commands")).unwrap();
        std::fs::write(
            root.join("skills").join("review.md"),
            "---\nname: review\ndescription: Reviews code\n---\nBe thorough.",
        )
        .unwrap();
        std::fs::write(
            root.join("commands").join("fix.md"),
            "---\nname: fix\ndescription: Fixes a bug\n---\nFix $ARGUMENTS",
        )
        .unwrap();

        let extensions = Extensions::load(&root, Path::new("/nonexistent"));
        assert_eq!(extensions.skills.len(), 1);
        assert_eq!(extensions.commands.len(), 1);
        assert_eq!(extensions.skill("review").unwrap().description, "Reviews code");
        assert_eq!(extensions.command("fix").unwrap().render("it"), "Fix it");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_project_skill_shadows_a_user_skill() {
        let project = temp_dir("shadow-p");
        let user = temp_dir("shadow-u");
        for root in [&project, &user] {
            std::fs::create_dir_all(root.join("skills")).unwrap();
            std::fs::write(
                root.join("skills").join("review.md"),
                format!("---\nname: review\ndescription: {}\n---\nbody", root.display()),
            )
            .unwrap();
        }
        let extensions = Extensions::load(&project, &user);
        assert_eq!(extensions.skills.len(), 1);
        assert!(extensions.skills[0].description.contains(&project.display().to_string()));
        std::fs::remove_dir_all(&project).ok();
        std::fs::remove_dir_all(&user).ok();
    }

    #[test]
    fn a_skill_directory_with_skill_md_is_loaded() {
        let root = temp_dir("skilldir");
        let dir = root.join("skills").join("browser");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), "---\nname: browser\n---\nUse the browser.").unwrap();

        let extensions = Extensions::load(&root, Path::new("/nonexistent"));
        assert_eq!(extensions.skills.len(), 1);
        assert_eq!(extensions.skills[0].name, "browser");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn hooks_need_an_event_and_a_command() {
        let root = temp_dir("hooks");
        std::fs::create_dir_all(root.join("hooks")).unwrap();
        std::fs::write(
            root.join("hooks").join("ok.md"),
            "---\nevent: pre_tool\ncommand: echo hi\ntools: bash, edit\n---\n",
        )
        .unwrap();
        std::fs::write(root.join("hooks").join("bad.md"), "---\nevent: pre_tool\n---\n").unwrap();

        let extensions = Extensions::load(&root, Path::new("/nonexistent"));
        assert_eq!(extensions.hooks.len(), 1);
        assert_eq!(extensions.hooks[0].event, HookEvent::PreTool);
        assert!(extensions.hooks[0].matches_tool("bash"));
        assert!(!extensions.hooks[0].matches_tool("read"));
        assert_eq!(extensions.problems.len(), 1);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_hook_without_a_tool_list_matches_everything() {
        let hook = Hook {
            event: HookEvent::PreTool,
            command: "true".into(),
            tools: vec![],
            path: PathBuf::new(),
        };
        assert!(hook.matches_tool("anything"));
    }

    #[test]
    fn plugins_load_from_a_manifest() {
        let root = temp_dir("plugins");
        let dir = root.join("plugins").join("demo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.json"),
            r#"{"name":"demo","description":"A demo","version":"1.0.0"}"#,
        )
        .unwrap();

        let extensions = Extensions::load(&root, Path::new("/nonexistent"));
        assert_eq!(extensions.plugins.len(), 1);
        assert_eq!(extensions.plugins[0].name, "demo");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_prompt_section_lists_skills() {
        let extensions = Extensions {
            skills: vec![Skill {
                name: "review".into(),
                description: "Reviews code".into(),
                body: String::new(),
                path: PathBuf::new(),
            }],
            ..Extensions::default()
        };
        let section = extensions.prompt_section();
        assert!(section.contains("review: Reviews code"));
    }

    #[test]
    fn an_empty_extension_set_adds_nothing_to_the_prompt() {
        assert!(Extensions::default().prompt_section().is_empty());
    }

    #[tokio::test]
    async fn a_hook_runs_and_returns_its_output() {
        let hook = Hook {
            event: HookEvent::SessionStart,
            command: "echo hello".into(),
            tools: vec![],
            path: PathBuf::new(),
        };
        let out = run_hook(&hook, Path::new("."), &serde_json::json!({})).await.unwrap();
        assert_eq!(out, "hello");
    }

    #[tokio::test]
    async fn a_failing_hook_reports_an_error() {
        let hook = Hook {
            event: HookEvent::SessionStart,
            command: "exit 3".into(),
            tools: vec![],
            path: PathBuf::new(),
        };
        assert!(run_hook(&hook, Path::new("."), &serde_json::json!({})).await.is_err());
    }
}
