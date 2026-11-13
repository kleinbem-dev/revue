use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::debug;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillSummary {
    pub name: String,
    pub description: String,
    pub triggers: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct LoadedSkill {
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub triggers: Vec<String>,
}

/// Discover the primary skills directory on this host via config, env, local path, or XDG/agent dirs.
pub fn find_skills_dir(configured_path: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = configured_path
        && !p.trim().is_empty()
    {
        let path = PathBuf::from(p);
        if path.is_dir() {
            return Some(path);
        }
    }

    if let Ok(env_path) = std::env::var("REVUE_SKILLS_DIR")
        && !env_path.trim().is_empty()
    {
        let path = PathBuf::from(env_path);
        if path.is_dir() {
            return Some(path);
        }
    }

    let local_skills = PathBuf::from("./skills");
    if local_skills.is_dir() {
        return Some(local_skills);
    }

    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
        && !xdg.trim().is_empty()
    {
        let xdg_skills = PathBuf::from(xdg).join("revue/skills");
        if xdg_skills.is_dir() {
            return Some(xdg_skills);
        }
    }

    if let Ok(home) = std::env::var("HOME")
        && !home.trim().is_empty()
    {
        let home_path = PathBuf::from(home);
        for rel in [
            ".config/revue/skills",
            ".gemini/config/skills",
            ".agents/skills",
        ] {
            let candidate = home_path.join(rel);
            if candidate.is_dir() {
                return Some(candidate);
            }
        }
    }

    None
}

/// Load a single skill from a directory containing a SKILL.md file
pub fn load_skill_from_dir(dir: &Path) -> Option<LoadedSkill> {
    let skill_file = dir.join("SKILL.md");
    if !skill_file.is_file() {
        return None;
    }

    let content = match std::fs::read_to_string(&skill_file) {
        Ok(c) => c,
        Err(_) => return None,
    };

    let dir_name = dir.file_name()?.to_string_lossy().to_string();
    let (name, description, body) = parse_frontmatter(&content, &dir_name);
    let triggers = default_triggers_for_skill(&name);

    // Limit body to top 150 lines so context window remains fast and crisp
    let trimmed_body: String = body.lines().take(150).collect::<Vec<_>>().join("\n");

    Some(LoadedSkill {
        name,
        description,
        instructions: trimmed_body,
        triggers,
    })
}

/// Parse YAML frontmatter (between `---` markers)
fn parse_frontmatter(content: &str, fallback_name: &str) -> (String, String, String) {
    let mut lines = content.lines();
    if lines.next() != Some("---") {
        return (
            fallback_name.to_string(),
            String::new(),
            content.to_string(),
        );
    }

    let mut name = fallback_name.to_string();
    let mut description = String::new();
    let mut in_frontmatter = true;
    let mut body_lines = Vec::new();

    for line in lines {
        if in_frontmatter {
            if line == "---" {
                in_frontmatter = false;
                continue;
            }
            if let Some(val) = line.strip_prefix("name:") {
                name = val.trim().to_string();
            } else if let Some(val) = line.strip_prefix("description:") {
                description = val.trim().to_string();
            }
        } else {
            body_lines.push(line);
        }
    }

    (name, description, body_lines.join("\n"))
}

/// Match patterns / extensions associated with known fleet skills
fn default_triggers_for_skill(skill_name: &str) -> Vec<String> {
    match skill_name {
        "svelte-core-bestpractices" | "svelte-code-writer" => vec![
            ".svelte".to_string(),
            ".svelte.ts".to_string(),
            ".svelte.js".to_string(),
        ],
        "kleinbem-fleet" => vec![
            ".nix".to_string(),
            "flake.lock".to_string(),
            "justfile".to_string(),
            "hosts/".to_string(),
            "modules/".to_string(),
        ],
        "home-assistant-best-practices" => vec![
            "hass".to_string(),
            "home-assistant".to_string(),
            "automations.yaml".to_string(),
            "scripts.yaml".to_string(),
        ],
        "jj-workflow" => vec![".jj".to_string(), "workspaces".to_string()],
        "agent-ops" => vec!["justfile".to_string(), ".github/workflows/".to_string()],
        "nixpkgs-packaging" | "nixos-org" | "nixpkgs-contributing" => vec![
            "pkgs/".to_string(),
            "package.nix".to_string(),
            "default.nix".to_string(),
        ],
        "mesh-llm-contributing" => {
            vec!["mesh".to_string(), "cuda".to_string(), "llama".to_string()]
        }
        _ => vec![skill_name.to_string()],
    }
}

/// Check if a skill should be activated for the given modified files
pub fn is_skill_triggered(skill: &LoadedSkill, changed_files: &[String]) -> bool {
    for trigger in &skill.triggers {
        for file in changed_files {
            let lower_file = file.to_lowercase();
            let lower_trigger = trigger.to_lowercase();

            if lower_trigger.starts_with('.') && lower_file.ends_with(&lower_trigger) {
                return true;
            }
            if lower_file.contains(&lower_trigger) {
                return true;
            }
        }
    }
    false
}

/// Find all matching skills for a review's changed files
pub fn find_matching_skills(
    skills_dir: Option<&Path>,
    changed_files: &[String],
) -> Vec<LoadedSkill> {
    let dir = match skills_dir {
        Some(d) => d,
        None => return Vec::new(),
    };

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut matched = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir()
            && let Some(skill) = load_skill_from_dir(&path)
            && is_skill_triggered(&skill, changed_files)
        {
            debug!("Activating fleet skill: {}", skill.name);
            matched.push(skill);
        }
    }

    matched
}

/// List all available skills in the skills directory (for Web API / UI inspection)
pub fn list_all_skills(skills_dir: Option<&Path>) -> Vec<SkillSummary> {
    let dir = match skills_dir {
        Some(d) => d,
        None => return Vec::new(),
    };

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut skills = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir()
            && let Some(skill) = load_skill_from_dir(&path)
        {
            skills.push(SkillSummary {
                name: skill.name,
                description: skill.description,
                triggers: skill.triggers,
            });
        }
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// Format loaded skills into an authoritative guidance block for prompt injection
pub fn format_skills_for_prompt(skills: &[LoadedSkill]) -> String {
    if skills.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    out.push_str("\nFLEET DOMAIN SKILLS & INVARIANTS (Enforce strictly):\n");
    out.push_str(
        "The following official team guidelines apply to the files modified in this review:\n\n",
    );

    for s in skills {
        out.push_str(&format!("--- BEGIN SKILL: {} ---\n", s.name));
        if !s.description.is_empty() {
            out.push_str(&format!("Overview: {}\n\n", s.description));
        }
        out.push_str(&s.instructions);
        out.push_str(&format!("\n--- END SKILL: {} ---\n\n", s.name));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_svelte_skill_trigger() {
        let skill = LoadedSkill {
            name: "svelte-core-bestpractices".to_string(),
            description: "test".to_string(),
            instructions: "test".to_string(),
            triggers: vec![".svelte".to_string(), ".svelte.ts".to_string()],
        };

        let files = vec!["web/src/routes/+page.svelte".to_string()];
        assert!(is_skill_triggered(&skill, &files));

        let rust_files = vec!["src/main.rs".to_string()];
        assert!(!is_skill_triggered(&skill, &rust_files));
    }

    #[test]
    fn test_nix_skill_trigger() {
        let skill = LoadedSkill {
            name: "kleinbem-fleet".to_string(),
            description: "test".to_string(),
            instructions: "test".to_string(),
            triggers: vec![".nix".to_string(), "flake.lock".to_string()],
        };

        let files = vec!["hosts/nixos-nvme/default.nix".to_string()];
        assert!(is_skill_triggered(&skill, &files));
    }
}
