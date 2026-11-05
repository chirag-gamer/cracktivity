//! One-click MCP client registration (FR-26): writes/merges the config
//! files of Claude Desktop, Claude Code, Codex, and Cursor — always with a
//! `.bak` backup, always preserving unknown keys (Claude Code's
//! `.claude.json` stores unrelated state; verified 2026-11-05, decisions
//! record). Paths are the Windows locations; absolute exe path (GUI clients
//! inherit a minimal PATH).

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    ClaudeDesktop,
    ClaudeCode,
    Codex,
    Cursor,
}

impl Client {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "claude_desktop" => Some(Client::ClaudeDesktop),
            "claude_code" => Some(Client::ClaudeCode),
            "codex" => Some(Client::Codex),
            "cursor" => Some(Client::Cursor),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Client::ClaudeDesktop => "Claude Desktop",
            Client::ClaudeCode => "Claude Code",
            Client::Codex => "Codex",
            Client::Cursor => "Cursor",
        }
    }
}

fn home() -> Result<PathBuf, String> {
    std::env::var("USERPROFILE")
        .map(PathBuf::from)
        .map_err(|_| "no USERPROFILE".into())
}

/// The config file this client reads (verified paths, see decisions).
pub fn config_path(client: Client) -> Result<PathBuf, String> {
    Ok(match client {
        Client::ClaudeDesktop => {
            // MSIX-virtualized installs keep it under LocalCache; prefer the
            // real one if both exist (research: prismix.dev guide).
            let roaming = std::env::var("APPDATA")
                .map(|a| {
                    PathBuf::from(a)
                        .join("Claude")
                        .join("claude_desktop_config.json")
                })
                .map_err(|_| "no APPDATA".to_string())?;
            if let Ok(packages) = std::env::var("LOCALAPPDATA") {
                let mut candidates = PathBuf::from(packages).join("Packages");
                if let Ok(entries) = std::fs::read_dir(&candidates) {
                    for e in entries.flatten() {
                        let candidate = e
                            .path()
                            .join("LocalCache\\Roaming\\Claude\\claude_desktop_config.json");
                        if candidate.exists() {
                            return Ok(candidate);
                        }
                    }
                }
                let _ = &mut candidates;
            }
            roaming
        }
        Client::ClaudeCode => home()?.join(".claude.json"),
        Client::Codex => home()?.join(".codex").join("config.toml"),
        Client::Cursor => home()?.join(".cursor").join("mcp.json"),
    })
}

/// The entry this app registers, per client config dialect.
fn json_entry(exe: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "stdio",
        "command": exe,
        "args": ["--mcp"],
    })
}

/// Registers (or updates) cracktivity in the client's config. Returns the
/// written path. The previous file is copied to `<path>.bak` first; JSON
/// merges preserve every unknown key; TOML appends the table.
pub fn register(client: Client, exe: &str) -> Result<PathBuf, String> {
    let path = config_path(client)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if path.exists() {
        std::fs::copy(&path, path.with_extension("json.bak"))
            .or_else(|_| std::fs::copy(&path, path.with_extension("toml.bak")))
            .map_err(|e| e.to_string())?;
    }
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let updated = match client {
        Client::ClaudeDesktop | Client::ClaudeCode | Client::Cursor => {
            merge_json_mcp_servers(&existing, exe)?
        }
        Client::Codex => merge_toml_mcp_server(&existing, exe),
    };
    std::fs::write(&path, updated).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Merges `mcpServers.cracktivity` into a JSON config, preserving all other
/// keys (top-level for Claude Desktop/Cursor and user-scope Claude Code).
fn merge_json_mcp_servers(existing: &str, exe: &str) -> Result<String, String> {
    let mut root: serde_json::Value = if existing.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(existing).map_err(|e| format!("config is not valid JSON: {e}"))?
    };
    let servers = root
        .as_object_mut()
        .ok_or("config root is not an object")?
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    servers
        .as_object_mut()
        .ok_or("mcpServers is not an object")?
        .insert("cracktivity".into(), json_entry(exe));
    serde_json::to_string_pretty(&root).map_err(|e| e.to_string())
}

/// Appends the `[mcp_servers.cracktivity]` table to Codex's config.toml.
/// A textual append (not a TOML rewrite) preserves every unrelated byte.
fn merge_toml_mcp_server(existing: &str, exe: &str) -> String {
    let mut out = String::from(existing);
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    // Idempotent: replace an existing table in place is not attempted —
    // the caller may re-run after editing; appending a duplicate table is
    // invalid TOML, so drop our previous section first.
    let lines: Vec<&str> = out.lines().collect();
    let mut kept: Vec<&str> = Vec::with_capacity(lines.len());
    let mut in_our_table = false;
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_our_table = trimmed == "[mcp_servers.cracktivity]";
            if in_our_table {
                continue;
            }
        }
        if !in_our_table {
            kept.push(line);
        }
    }
    let mut result = kept.join("\n");
    if !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }
    result.push_str(&format!(
        "\n[mcp_servers.cracktivity]\ncommand = \"{exe}\"\nargs = [\"--mcp\"]\n"
    ));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_merge_preserves_unknown_keys() {
        let existing = r#"{
  "projects": { "D:\\work": { "history": [1, 2, 3] } },
  "mcpServers": { "other": { "command": "x" } }
}"#;
        let merged = merge_json_mcp_servers(existing, "C:\\apps\\cracktivity.exe").unwrap();
        let v: serde_json::Value = serde_json::from_str(&merged).unwrap();
        // Unknown top-level state survives…
        assert!(v["projects"]["D:\\work"]["history"].is_array());
        // …as does the unrelated sibling server…
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        // …and ours is exactly the stdio registration.
        assert_eq!(
            v["mcpServers"]["cracktivity"]["command"],
            "C:\\apps\\cracktivity.exe"
        );
        assert_eq!(v["mcpServers"]["cracktivity"]["args"][0], "--mcp");
        assert_eq!(v["mcpServers"]["cracktivity"]["type"], "stdio");
    }

    #[test]
    fn json_merge_from_empty_file() {
        let merged = merge_json_mcp_servers("", "x.exe").unwrap();
        let v: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(v["mcpServers"]["cracktivity"]["args"][0], "--mcp");
    }

    #[test]
    fn toml_merge_appends_and_is_idempotent() {
        let existing = "[profile.default]\nmodel = \"gpt-5\"\n";
        let once = merge_toml_mcp_server(existing, "C:\\cracktivity.exe");
        assert!(once.contains("[profile.default]"));
        assert!(once.contains("[mcp_servers.cracktivity]"));
        assert!(once.contains(r#"command = "C:\cracktivity.exe""#));
        // Re-running does not duplicate the table.
        let twice = merge_toml_mcp_server(&once, "C:\\cracktivity.exe");
        assert_eq!(
            twice.matches("[mcp_servers.cracktivity]").count(),
            1,
            "{twice}"
        );
        assert!(twice.contains("model = \"gpt-5\""));
    }

    #[test]
    fn codex_toml_merge() {
        // The Codex entry shape: [mcp_servers.<name>] with command/args.
        let once = merge_toml_mcp_server("", "cracktivity.exe");
        assert!(once.contains("args = [\"--mcp\"]"));
    }
}
