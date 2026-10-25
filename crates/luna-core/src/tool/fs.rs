//! Filesystem tools: read, write, edit, multi_edit, ls, glob, grep.

use std::path::Path;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::{
    optional_bool, optional_str, optional_usize, required_str, Tool, ToolClass, ToolContext,
    ToolOutput,
};
use crate::error::{LunaError, Result};

/// Files above this size are refused rather than silently truncated, because a
/// partial file is worse than a clear error.
const MAX_READ_BYTES: u64 = 4 * 1024 * 1024;

fn read_text(path: &Path) -> Result<String> {
    let metadata = std::fs::metadata(path)
        .map_err(|source| LunaError::Io { path: path.to_path_buf(), source })?;
    if metadata.len() > MAX_READ_BYTES {
        return Err(LunaError::tool(format!(
            "{} is {} bytes; the limit is {MAX_READ_BYTES}",
            path.display(),
            metadata.len()
        )));
    }
    std::fs::read_to_string(path)
        .map_err(|source| LunaError::Io { path: path.to_path_buf(), source })
}

/// Render a file with line numbers, the way a model reads it best.
fn numbered(content: &str, offset: usize, limit: Option<usize>) -> String {
    let mut out = String::new();
    let take = limit.unwrap_or(usize::MAX);
    for (index, line) in content.lines().enumerate().skip(offset).take(take) {
        out.push_str(&format!("{:>6}\t{line}\n", index + 1));
    }
    if out.is_empty() {
        out.push_str("(the file is empty)\n");
    }
    out
}

pub struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read a text file. Returns the contents with line numbers so you can cite \
         exact lines. Use `offset` and `limit` for large files."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path, absolute or relative to the working directory" },
                "offset": { "type": "integer", "description": "0-based line to start at" },
                "limit": { "type": "integer", "description": "Maximum number of lines to return" }
            },
            "required": ["path"]
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let path = ctx.resolve(&required_str(&args, "path")?)?;
        let content = read_text(&path)?;
        let offset = optional_usize(&args, "offset").unwrap_or(0);
        let limit = optional_usize(&args, "limit");
        Ok(ToolOutput::ok(numbered(&content, offset, limit)))
    }
}

pub struct WriteTool;

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Create a file, or replace its entire contents. Parent directories are \
         created as needed. Prefer `edit` for changing part of an existing file."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "content": { "type": "string" }
            },
            "required": ["path", "content"]
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Write
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let path = ctx.resolve(&required_str(&args, "path")?)?;
        let content = required_str(&args, "content")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|source| LunaError::Io { path: parent.to_path_buf(), source })?;
        }
        let existed = path.exists();
        crate::config::write_atomic(&path, content.as_bytes())?;
        let lines = content.lines().count();
        Ok(ToolOutput::ok(format!(
            "{} {} ({lines} lines)",
            if existed { "wrote" } else { "created" },
            path.display()
        )))
    }
}

/// Replace exactly one occurrence of `old` with `new`.
fn replace_once(content: &str, old: &str, new: &str) -> Result<String> {
    if old.is_empty() {
        return Err(LunaError::tool("`old` must not be empty"));
    }
    let matches = content.matches(old).count();
    match matches {
        0 => Err(LunaError::tool(
            "`old` was not found in the file; read it again and copy the exact text",
        )),
        1 => Ok(content.replacen(old, new, 1)),
        count => Err(LunaError::tool(format!(
            "`old` appears {count} times; include more surrounding context so the edit is unambiguous"
        ))),
    }
}

pub struct EditTool;

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Replace one exact occurrence of `old` with `new` in a file. `old` must \
         match the file byte for byte, including indentation, and must be unique."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "old": { "type": "string", "description": "Exact text to replace" },
                "new": { "type": "string", "description": "Replacement text" }
            },
            "required": ["path", "old", "new"]
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Write
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let path = ctx.resolve(&required_str(&args, "path")?)?;
        let old = required_str(&args, "old")?;
        let new = required_str(&args, "new")?;
        let content = read_text(&path)?;
        // A bad match is the model's mistake to fix, not a crash: report it as a
        // tool error so it can read the message and retry.
        let updated = match replace_once(&content, &old, &new) {
            Ok(updated) => updated,
            Err(err) => return Ok(ToolOutput::error(err.to_string())),
        };
        crate::config::write_atomic(&path, updated.as_bytes())?;
        Ok(ToolOutput::ok(format!("edited {}", path.display())))
    }
}

pub struct MultiEditTool;

#[async_trait]
impl Tool for MultiEditTool {
    fn name(&self) -> &str {
        "multi_edit"
    }

    fn description(&self) -> &str {
        "Apply several exact replacements to one file in a single call. Edits are \
         applied in order; if any fails, nothing is written."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "edits": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "old": { "type": "string" },
                            "new": { "type": "string" }
                        },
                        "required": ["old", "new"]
                    }
                }
            },
            "required": ["path", "edits"]
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Write
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let path = ctx.resolve(&required_str(&args, "path")?)?;
        let edits = args
            .get("edits")
            .and_then(|edits| edits.as_array())
            .ok_or_else(|| LunaError::tool("`edits` must be an array"))?;
        let mut content = read_text(&path)?;
        for (index, edit) in edits.iter().enumerate() {
            let old = required_str(edit, "old")
                .map_err(|err| LunaError::tool(format!("edit {index}: {err}")))?;
            let new = required_str(edit, "new")
                .map_err(|err| LunaError::tool(format!("edit {index}: {err}")))?;
            content = match replace_once(&content, &old, &new) {
                Ok(updated) => updated,
                Err(err) => return Ok(ToolOutput::error(format!("edit {index}: {err}"))),
            };
        }
        crate::config::write_atomic(&path, content.as_bytes())?;
        Ok(ToolOutput::ok(format!("applied {} edits to {}", edits.len(), path.display())))
    }
}

pub struct LsTool;

#[async_trait]
impl Tool for LsTool {
    fn name(&self) -> &str {
        "ls"
    }

    fn description(&self) -> &str {
        "List a directory. Directories end with a slash. Hidden entries are \
         omitted unless `all` is true."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Directory, defaults to the working directory" },
                "all": { "type": "boolean", "description": "Include hidden entries" }
            }
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let path = match optional_str(&args, "path") {
            Some(path) => ctx.resolve(&path)?,
            None => ctx.cwd.clone(),
        };
        let all = optional_bool(&args, "all").unwrap_or(false);
        let mut entries: Vec<String> = Vec::new();
        let reader = std::fs::read_dir(&path)
            .map_err(|source| LunaError::Io { path: path.clone(), source })?;
        for entry in reader.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !all && name.starts_with('.') {
                continue;
            }
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            entries.push(if is_dir { format!("{name}/") } else { name });
        }
        entries.sort();
        if entries.is_empty() {
            return Ok(ToolOutput::ok(format!("{} is empty", path.display())));
        }
        Ok(ToolOutput::ok(entries.join("\n")))
    }
}

pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files by glob pattern, e.g. `**/*.rs` or `src/**/*.ts`. Results are \
         sorted by modification time, newest first."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string" },
                "path": { "type": "string", "description": "Directory to search from" }
            },
            "required": ["pattern"]
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let pattern = required_str(&args, "pattern")?;
        let root = match optional_str(&args, "path") {
            Some(path) => ctx.resolve(&path)?,
            None => ctx.cwd.clone(),
        };
        let full = root.join(&pattern).to_string_lossy().to_string();
        let mut found: Vec<(std::time::SystemTime, String)> = Vec::new();
        let paths = glob::glob(&full).map_err(|err| LunaError::tool(err.to_string()))?;
        for entry in paths.flatten() {
            if entry.is_dir() {
                continue;
            }
            let modified =
                entry.metadata().and_then(|meta| meta.modified()).unwrap_or(std::time::UNIX_EPOCH);
            found.push((modified, entry.display().to_string()));
        }
        found.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        if found.is_empty() {
            return Ok(ToolOutput::ok(format!("no files matched `{pattern}`")));
        }
        let listing: Vec<String> = found.into_iter().take(500).map(|(_, path)| path).collect();
        Ok(ToolOutput::ok(listing.join("\n")))
    }
}

pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search file contents with a regular expression. Respects .gitignore and \
         skips binary files. Returns `path:line: text` matches."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Regular expression" },
                "path": { "type": "string", "description": "Directory or file to search" },
                "glob": { "type": "string", "description": "Only search files matching this glob, e.g. `*.rs`" },
                "ignore_case": { "type": "boolean" },
                "max_results": { "type": "integer", "description": "Defaults to 200" }
            },
            "required": ["pattern"]
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let pattern = required_str(&args, "pattern")?;
        let root = match optional_str(&args, "path") {
            Some(path) => ctx.resolve(&path)?,
            None => ctx.cwd.clone(),
        };
        let ignore_case = optional_bool(&args, "ignore_case").unwrap_or(false);
        let max_results = optional_usize(&args, "max_results").unwrap_or(200);
        let file_glob = optional_str(&args, "glob");

        let regex = regex::RegexBuilder::new(&pattern)
            .case_insensitive(ignore_case)
            .build()
            .map_err(|err| LunaError::tool(format!("invalid regular expression: {err}")))?;

        let mut matches: Vec<String> = Vec::new();
        let mut walker = ignore::WalkBuilder::new(&root);
        walker.hidden(true).git_ignore(true).git_global(true).git_exclude(true).parents(true);
        if let Some(glob) = &file_glob {
            let mut overrides = ignore::overrides::OverrideBuilder::new(&root);
            overrides
                .add(glob)
                .map_err(|err| LunaError::tool(format!("invalid glob `{glob}`: {err}")))?;
            let built = overrides
                .build()
                .map_err(|err| LunaError::tool(format!("invalid glob `{glob}`: {err}")))?;
            walker.overrides(built);
        }

        'outer: for entry in walker.build().flatten() {
            if ctx.cancel.is_cancelled() {
                return Err(LunaError::Cancelled);
            }
            let path = entry.path();
            if !entry.file_type().map(|kind| kind.is_file()).unwrap_or(false) {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(path) else {
                // Binary or unreadable; skip rather than fail the whole search.
                continue;
            };
            for (index, line) in content.lines().enumerate() {
                if regex.is_match(line) {
                    matches.push(format!("{}:{}: {}", path.display(), index + 1, line.trim_end()));
                    if matches.len() >= max_results {
                        break 'outer;
                    }
                }
            }
        }

        if matches.is_empty() {
            return Ok(ToolOutput::ok(format!("no matches for `{pattern}`")));
        }
        Ok(ToolOutput::ok(matches.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("luna-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn write_then_read_round_trips() {
        let dir = temp_dir("rw");
        let ctx = ToolContext::new(&dir);
        let write =
            WriteTool.run(json!({"path": "a.txt", "content": "one\ntwo\n"}), &ctx).await.unwrap();
        assert!(!write.is_error);

        let read = ReadTool.run(json!({"path": "a.txt"}), &ctx).await.unwrap();
        assert!(read.content.contains("one"));
        assert!(read.content.contains("     2\t"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn edit_requires_a_unique_match() {
        let dir = temp_dir("edit");
        let ctx = ToolContext::new(&dir);
        WriteTool.run(json!({"path": "b.txt", "content": "x\nx\n"}), &ctx).await.unwrap();

        let ambiguous =
            EditTool.run(json!({"path": "b.txt", "old": "x", "new": "y"}), &ctx).await.unwrap();
        assert!(ambiguous.is_error);
        assert!(ambiguous.content.contains("appears 2 times"));

        let missing =
            EditTool.run(json!({"path": "b.txt", "old": "zzz", "new": "y"}), &ctx).await.unwrap();
        assert!(missing.is_error);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn multi_edit_is_all_or_nothing() {
        let dir = temp_dir("multiedit");
        let ctx = ToolContext::new(&dir);
        WriteTool.run(json!({"path": "c.txt", "content": "alpha beta"}), &ctx).await.unwrap();

        let failed = MultiEditTool
            .run(
                json!({"path": "c.txt", "edits": [
                    {"old": "alpha", "new": "ALPHA"},
                    {"old": "missing", "new": "x"}
                ]}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(failed.is_error);
        let content = std::fs::read_to_string(dir.join("c.txt")).unwrap();
        assert_eq!(content, "alpha beta");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn grep_finds_matches_with_line_numbers() {
        let dir = temp_dir("grep");
        let ctx = ToolContext::new(&dir);
        WriteTool
            .run(json!({"path": "d.rs", "content": "fn main() {}\nlet x = 1;\n"}), &ctx)
            .await
            .unwrap();
        let found = GrepTool.run(json!({"pattern": "fn main"}), &ctx).await.unwrap();
        assert!(found.content.contains(":1:"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn glob_lists_matching_files() {
        let dir = temp_dir("glob");
        let ctx = ToolContext::new(&dir);
        WriteTool.run(json!({"path": "e.rs", "content": ""}), &ctx).await.unwrap();
        let found = GlobTool.run(json!({"pattern": "*.rs"}), &ctx).await.unwrap();
        assert!(found.content.contains("e.rs"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn replace_once_rejects_empty_needles() {
        assert!(replace_once("abc", "", "x").is_err());
    }
}
