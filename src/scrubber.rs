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
        // 3. AWS Access Key IDs
        (
            "AWS_ACCESS_KEY",
            Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap(),
        ),
        // 4. AWS Secret Access Keys
        (
            "AWS_SECRET_KEY",
            Regex::new(r#"(?i)\b(?:aws_secret_access_key|aws_secret_key)\b\s*[:=]\s*["']?([A-Za-z0-9/+=]{40})["']?"#)
                .unwrap(),
        ),
        // 5. Google API / AI Studio Keys
        (
            "GOOGLE_API_KEY",
            Regex::new(r"\bAIza[0-9A-Za-z\-_]{35}\b").unwrap(),
        ),
        // 6. Anthropic API Keys
        (
            "ANTHROPIC_KEY",
            Regex::new(r"\bsk-ant-[a-zA-Z0-9_\-]{32,}\b").unwrap(),
        ),
        // 7. OpenAI API Keys (legacy sk-... and modern sk-proj-... / sk-admin-...)
        (
            "OPENAI_KEY",
            Regex::new(r"\bsk-(?:proj-|admin-)?[a-zA-Z0-9_\-]{20,}\b").unwrap(),
        ),
        // 8. Slack Tokens
        (
            "SLACK_TOKEN",
            Regex::new(r"\bxox[baprs]-[0-9a-zA-Z]{10,48}\b").unwrap(),
        ),
        // 9. Database / Service URI Credentials (e.g. postgres://user:password@host)
        (
            "URI_CREDENTIALS",
            Regex::new(r"(?i)([a-z]+://[^:\s]+:)([^@\s/]+)(@[^/\s]+)").unwrap(),
        ),
        // 10. Generic key/password assignments in diffs (quoted JSON/YAML, unquoted .env, or assignments)
        (
            "CREDENTIAL_ASSIGNMENT",
            Regex::new(r#"(?i)(?:"(?:password|secret_key|api_key|auth_token|client_secret|access_token|private_key|token)"|'(?:password|secret_key|api_key|auth_token|client_secret|access_token|private_key|token)'|\b(?:password|secret_key|api_key|auth_token|client_secret|access_token|private_key|token)\b)\s*[:=]\s*(?:"([^"\\]*(?:\\.[^"\\]*)*)"|'([^'\\]*(?:\\.[^'\\]*)*)'|([^"'\s\r\n]{8,}))"#)
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
        } else if *name == "AWS_SECRET_KEY" {
            let matches: Vec<_> = re.find_iter(&sanitized).collect();
            if !matches.is_empty() {
                total_redacted += matches.len();
                sanitized = re
                    .replace_all(&sanitized, |caps: &regex::Captures| {
                        let m0 = caps.get(0).unwrap();
                        let m = caps.get(1).unwrap();
                        let full = m0.as_str();
                        let s = m.start() - m0.start();
                        let e = m.end() - m0.start();
                        format!("{}[REDACTED_SECRET:AWS_SECRET_KEY]{}", &full[..s], &full[e..])
                    })
                    .to_string();
            }
        } else if *name == "CREDENTIAL_ASSIGNMENT" {
            let mut count = 0;
            sanitized = re
                .replace_all(&sanitized, |caps: &regex::Captures| {
                    let m0 = caps.get(0).unwrap();
                    let m = caps
                        .get(1)
                        .or_else(|| caps.get(2))
                        .or_else(|| caps.get(3))
                        .unwrap();
                    let val = m.as_str().trim();
                    let is_exact_marker = val
                        .strip_prefix("[REDACTED_SECRET:")
                        .and_then(|inner| inner.strip_suffix(']'))
                        .map(|name| {
                            !name.is_empty()
                                && name
                                    .chars()
                                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                        })
                        .unwrap_or(false);
                    if is_exact_marker {
                        return m0.as_str().to_string();
                    }
                    count += 1;
                    let full = m0.as_str();
                    let s = m.start() - m0.start();
                    let e = m.end() - m0.start();
                    format!("{}[REDACTED_SECRET:CREDENTIAL]{}", &full[..s], &full[e..])
                })
                .to_string();
            total_redacted += count;
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
        info!(
            "DLP Guard: Redacted {} sensitive secret(s) from diff prior to LLM analysis",
            total_redacted
        );
    }

    (sanitized, total_redacted)
}

/// Convenience helper to redact sensitive secrets from arbitrary text (e.g. PR descriptions, comment threads).
pub fn scrub_text(text: &str) -> String {
    let (scrubbed, _) = scrub_diff(text);
    scrubbed
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
            "+const OPENAI_PROJ = \"sk-proj-1234567890abcdefghijklmnopqrstuvwxyz1234567890\";\n", // gitleaks:allow
        );
        let (scrubbed, count) = scrub_diff(diff);
        assert_eq!(count, 4);
        assert!(scrubbed.contains("[REDACTED_SECRET:GITHUB_TOKEN]"));
        assert!(scrubbed.contains("[REDACTED_SECRET:GOOGLE_API_KEY]"));
        assert!(scrubbed.contains("[REDACTED_SECRET:AWS_ACCESS_KEY]"));
        assert!(scrubbed.contains("[REDACTED_SECRET:OPENAI_KEY]"));
    }

    #[test]
    fn test_scrub_uri_credentials() {
        let diff = r#"+DATABASE_URL="postgres://admin:super_secret_password_123@db.example.com:5432/revue""#;
        let (scrubbed, count) = scrub_diff(diff);
        assert_eq!(count, 1);
        assert!(scrubbed
            .contains("postgres://admin:[REDACTED_SECRET:URI_PASSWORD]@db.example.com:5432/revue"));
        assert!(!scrubbed.contains("super_secret_password_123"));
    }

    #[test]
    fn test_scrub_aws_secret_key() {
        let text = "aws_secret_access_key = \"wJalrXUtnFEMI/K7MDENG/bPxRfiCY1234567890\""; // gitleaks:allow
        let (scrubbed, count) = scrub_diff(text);
        assert_eq!(count, 1);
        assert!(scrubbed.contains("aws_secret_access_key = \"[REDACTED_SECRET:AWS_SECRET_KEY]\""));
    }

    #[test]
    fn test_scrub_quoted_json_and_env_assignments() {
        let json_diff = r#"+  "password": "super_secret_cleartext_password_123","#;
        let (scrubbed_json, count) = scrub_diff(json_diff);
        assert_eq!(count, 1);
        assert!(scrubbed_json.contains(r#"+  "password": "[REDACTED_SECRET:CREDENTIAL]","#));

        let phrase_diff = r#"+password: "my secret pass phrase with spaces""#; // gitleaks:allow
        let (scrubbed_phrase, count) = scrub_diff(phrase_diff);
        assert_eq!(count, 1);
        assert_eq!(scrubbed_phrase, r#"+password: "[REDACTED_SECRET:CREDENTIAL]""#);

        let substring_diff = r#"+secret_key: "secret_key_12345678""#; // gitleaks:allow
        let (scrubbed_sub, count) = scrub_diff(substring_diff);
        assert_eq!(count, 1);
        assert_eq!(scrubbed_sub, r#"+secret_key: "[REDACTED_SECRET:CREDENTIAL]""#);

        let token_diff = r#"+token: "custom_auth_token_xyz""#; // gitleaks:allow
        let (scrubbed_token, count) = scrub_diff(token_diff);
        assert_eq!(count, 1);
        assert_eq!(scrubbed_token, r#"+token: "[REDACTED_SECRET:CREDENTIAL]""#);

        let env_diff = "+API_KEY=my_secret_env_token_value_xyz\n"; // gitleaks:allow
        let (scrubbed_env, count) = scrub_diff(env_diff);
        assert_eq!(count, 1);
        assert!(scrubbed_env.contains("+API_KEY=[REDACTED_SECRET:CREDENTIAL]\n"));
    }

    #[test]
    fn test_scrub_text_helper() {
        let text = "PR contains client_secret: 'super_secret_token_12345' in config";
        let clean = scrub_text(text);
        assert!(clean.contains("[REDACTED_SECRET:CREDENTIAL]"));
        assert!(!clean.contains("super_secret_token_12345"));
    }

    #[test]
    fn test_scrub_partial_marker_value() {
        let text = r#"password: "[REDACTED_SECRET:LEAK] and a real secret phrase""#; // gitleaks:allow
        let (clean, count) = scrub_diff(text);
        assert_eq!(count, 1);
        assert_eq!(clean, r#"password: "[REDACTED_SECRET:CREDENTIAL]""#);

        let marker_shaped = r#"password: "[REDACTED_SECRET:x]secret]""#; // gitleaks:allow
        let (clean2, count2) = scrub_diff(marker_shaped);
        assert_eq!(count2, 1);
        assert_eq!(clean2, r#"password: "[REDACTED_SECRET:CREDENTIAL]""#);
    }
}
