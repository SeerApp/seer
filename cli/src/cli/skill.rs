use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub(super) const MARKDOWN: &str = include_str!("skill.md");

const DIRS: [&str; 4] = [
    ".cursor/skills/seer",
    ".claude/skills/seer",
    ".codex/skills/seer",
    ".agents/skills/seer",
];

pub(super) fn cmd(install: bool, short: bool) -> Result<()> {
    if !install {
        print!("{MARKDOWN}");
        return Ok(());
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .context("no home directory")?;
    let (written, errors) = install_into(&home);
    let value = serde_json::json!({
        "written": written,
        "errors": errors.iter().map(|(path, error)| serde_json::json!({"path": path, "error": error})).collect::<Vec<_>>(),
    });
    super::present::emit(value, &["seer skill".into()], short)?;
    if written.is_empty() {
        anyhow::bail!("wrote no skill files");
    }
    Ok(())
}

pub(super) fn install_into(home: &Path) -> (Vec<String>, Vec<(String, String)>) {
    let mut written = Vec::new();
    let mut errors = Vec::new();
    for rel in DIRS {
        let dir = home.join(rel);
        let path = dir.join("SKILL.md");
        let show = path.display().to_string();
        match std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, MARKDOWN)) {
            Ok(()) => written.push(show),
            Err(err) => errors.push((show, err.to_string())),
        }
    }
    (written, errors)
}
