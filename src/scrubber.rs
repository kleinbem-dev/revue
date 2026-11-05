use regex::Regex;
use std::sync::LazyLock;
use tracing::info;

pub struct RedactionRule {
    pub name: &'static str,
    pub regex: LazyLock<Regex>,
}

static RULES: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    vec![
        // 1. Private Keys
        (
            "PRIVATE_KEY",
            Regex::new(r"(?s)-----BEGIN [A-Z0-9 ]+PRIVATE KEY-----.*?-----END [A-Z0-9 ]+PRIVATE KEY-----")
                .unwrap(),
        ),
        // 2. GitHub Personal Access & App Tokens
        (
            "GITHUB_TOKEN",
            Regex::new(r"\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9_]{36,255}\b|\bgithub_pat_[A-Za-z0-9_]{82}\b")
                .unwrap(),
        ),
        // 3. AWS Access Key IDs & Secret Access Keys
        (
            "AWS_ACCESS_KEY",
            Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap(),
        ),
        // 4. Google API / AI Studio Keys
        (
            "GOOGLE_API_KEY",
            Regex::new(r"\bAIza[0-9A-Za-z\-_]{35}\b").unwrap(),
        ),
        // 5. Anthropic API Keys
        (
            "ANTHROPIC_KEY",
            Regex::new(r"\bsk-ant-[a-zA-Z0-9_\-]{32,}\b").unwrap(),
        ),
        // 6. OpenAI API Keys
        (
            "OPENAI_KEY",
            Regex::new(r"\bsk-[a-zA-Z0-9]{32,}\b").unwrap(),
        ),
        // 7. Slack Tokens
        (
            "SLACK_TOKEN",
            Regex::new(r"\bxox[baprs]-[0-9a-zA-Z]{10,48}\b").unwrap(),
        ),
        // 8. Database / Service URI Credentials (e.g. postgres://user:password@host)
        (
            "URI_CREDENTIALS",
            Regex::new(r"(?i)([a-z]+://[^:\s]+:)([^@\s/]+)(@[^/\s]+)").unwrap(),
        ),
        // 9. Generic key/password assignments in diffs (e.g. password = "secret_value")
        (
            "CREDENTIAL_ASSIGNMENT",
            Regex::new(r#"(?i)(password|secret_key|api_key|auth_token|client_secret)\s*[:=]\s*["']([^"'\s]{8,})["']"#)
                .unwrap(),
        ),
    ]
});

/// Redacts sensitive credentials, private keys, and tokens from diff text before dispatching to LLM providers.
/// Returns the sanitized string and the number of secrets redacted.
pub fn scrub_diff(diff: &str) -> (String, usize) {
    let mut sanitized = diff.to_string();
    let mut total_redacted = 0;

    for (name, re) in RULES.iter() {
        if *name == "URI_CREDENTIALS" {
            let matches: Vec<_> = re.find_iter(&sanitized).collect();
            if !matches.is_empty() {
                total_redacted += matches.len();
                sanitized = re
                    .replace_all(&sanitized, |caps: &regex::Captures| {
                        format!("{}[REDACTED_SECRET:URI_PASSWORD]{}", &caps[1], &caps[3])
                    })
                    .to_string();
            }
        } else if *name == "CREDENTIAL_ASSIGNMENT" {
            let matches: Vec<_> = re.find_iter(&sanitized).collect();
            if !matches.is_empty() {
                total_redacted += matches.len();
                sanitized = re
                    .replace_all(&sanitized, |caps: &regex::Captures| {
                        format!("{}: \"[REDACTED_SECRET:CREDENTIAL]\"", &caps[1])
                    })
                    .to_string();
            }
        } else {
            let matches: Vec<_> = re.find_iter(&sanitized).collect();
            if !matches.is_empty() {
                total_redacted += matches.len();
                let replacement = format!("[REDACTED_SECRET:{}]", name);
                sanitized = re.replace_all(&sanitized, replacement.as_str()).to_string();
            }
        }
    }

    if total_redacted > 0 {
        info!("DLP Guard: Redacted {} sensitive secret(s) from diff prior to LLM analysis", total_redacted);
    }

    (sanitized, total_redacted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrub_private_keys() {
        let diff = r#"
+-----BEGIN RSA PRIVATE KEY-----
+MIIEowIBAAKCAQEA0Y3L
+-----END RSA PRIVATE KEY-----
"#;
        let (scrubbed, count) = scrub_diff(diff);
        assert_eq!(count, 1);
        assert!(scrubbed.contains("[REDACTED_SECRET:PRIVATE_KEY]"));
        assert!(!scrubbed.contains("MIIEowIBAAKCAQEA0Y3L"));
    }

    #[test]
    fn test_scrub_api_keys() {
        // Made-up keys in the shapes the scrubber matches; concat! leaves room
        // for the gitleaks:allow markers without changing the diff.
        let diff = concat!(
            "\n",
            "+const GITHUB_TOKEN = \"ghp_1234567890abcdefghijklmnopqrstuvwxyz1234\";\n",
            "+const GOOGLE_KEY = \"AIzaSyD-1234567890abcdefghijklmnopqrstu\";\n", // gitleaks:allow
            "+const AWS_KEY = \"AKIA1234567890ABCDEF\";\n",                       // gitleaks:allow
        );
        let (scrubbed, count) = scrub_diff(diff);
        assert_eq!(count, 3);
        assert!(scrubbed.contains("[REDACTED_SECRET:GITHUB_TOKEN]"));
        assert!(scrubbed.contains("[REDACTED_SECRET:GOOGLE_API_KEY]"));
        assert!(scrubbed.contains("[REDACTED_SECRET:AWS_ACCESS_KEY]"));
    }

    #[test]
    fn test_scrub_uri_credentials() {
        let diff = r#"+DATABASE_URL="postgres://admin:super_secret_password_123@db.example.com:5432/revue""#;
        let (scrubbed, count) = scrub_diff(diff);
        assert_eq!(count, 1);
        assert!(scrubbed.contains("postgres://admin:[REDACTED_SECRET:URI_PASSWORD]@db.example.com:5432/revue"));
        assert!(!scrubbed.contains("super_secret_password_123"));
    }
}
