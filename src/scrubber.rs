use regex::{Captures, Regex};
use std::sync::LazyLock;
use tracing::info;

enum RuleReplacement {
    Marker(&'static str),
    UriPassword,
    AwsSecret,
    CredentialAssignment,
}

static RULES: LazyLock<Vec<(Regex, RuleReplacement)>> = LazyLock::new(|| {
    vec![
        (
            Regex::new(
                r"(?s)-----BEGIN [A-Z0-9 ]+PRIVATE KEY-----.*?-----END [A-Z0-9 ]+PRIVATE KEY-----",
            )
            .unwrap(),
            RuleReplacement::Marker("[REDACTED_SECRET:PRIVATE_KEY]"),
        ),
        (
            Regex::new(
                r"\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9_]{36,255}\b|\bgithub_pat_[A-Za-z0-9_]{82}\b",
            )
            .unwrap(),
            RuleReplacement::Marker("[REDACTED_SECRET:GITHUB_TOKEN]"),
        ),
        (
            Regex::new(r"\bglpat-[0-9a-zA-Z_\-]{20,}\b").unwrap(),
            RuleReplacement::Marker("[REDACTED_SECRET:GITLAB_TOKEN]"),
        ),
        (
            Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap(),
            RuleReplacement::Marker("[REDACTED_SECRET:AWS_ACCESS_KEY]"),
        ),
        (
            Regex::new(
                r#"(?i)\b(?:aws_secret_access_key|aws_secret_key)\b\s*[:=]\s*["']?([A-Za-z0-9/+=]{40})["']?"#,
            )
            .unwrap(),
            RuleReplacement::AwsSecret,
        ),
        (
            Regex::new(r"\bAIza[0-9A-Za-z\-_]{35}\b").unwrap(),
            RuleReplacement::Marker("[REDACTED_SECRET:GOOGLE_API_KEY]"),
        ),
        (
            Regex::new(r"\bsk-ant-[a-zA-Z0-9_\-]{32,}\b").unwrap(),
            RuleReplacement::Marker("[REDACTED_SECRET:ANTHROPIC_KEY]"),
        ),
        (
            Regex::new(r"\bsk-(?:proj-|admin-)?[a-zA-Z0-9_\-]{20,}\b").unwrap(),
            RuleReplacement::Marker("[REDACTED_SECRET:OPENAI_KEY]"),
        ),
        (
            Regex::new(r"\bxox[baprs]-[0-9a-zA-Z\-]{10,128}\b").unwrap(),
            RuleReplacement::Marker("[REDACTED_SECRET:SLACK_TOKEN]"),
        ),
        (
            Regex::new(r"(?i)\bBearer\s+eyJ[A-Za-z0-9_\-]+\.eyJ[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+")
                .unwrap(),
            RuleReplacement::Marker("Bearer [REDACTED_SECRET:JWT]"),
        ),
        (
            Regex::new(r"(?i)([a-z]+://[^:\s]+:)([^@\s/]+)(@[^/\s]+)").unwrap(),
            RuleReplacement::UriPassword,
        ),
        (
            Regex::new(
                r#"(?i)(?:"(?:password|secret_key|api_key|auth_token|client_secret|access_token|private_key|token)"|'(?:password|secret_key|api_key|auth_token|client_secret|access_token|private_key|token)'|\b(?:password|secret_key|api_key|auth_token|client_secret|access_token|private_key|token)\b)\s*[:=]\s*(?:"([^"\\]*(?:\\.[^"\\]*)*)"|'([^'\\]*(?:\\.[^"\\]*)*)'|([^"'\s\r\n]{8,}))"#,
            )
            .unwrap(),
            RuleReplacement::CredentialAssignment,
        ),
    ]
});

fn is_complete_marker(v: &str) -> bool {
    v.starts_with("[REDACTED_SECRET:")
        && v.ends_with(']')
        && v.len() > "[REDACTED_SECRET:]".len()
        && v["[REDACTED_SECRET:".len()..v.len() - 1]
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b == b'_')
}

/// Redacts credentials, private keys, and tokens from text before dispatching to LLM providers.
/// Returns the sanitized string and the number of secrets redacted.
pub fn scrub_diff(diff: &str) -> (String, usize) {
    let mut sanitized = diff.to_string();
    let mut total_redacted = 0;

    for (re, kind) in RULES.iter() {
        let mut rule_hits = 0usize;
        let replaced = match kind {
            RuleReplacement::Marker(marker) => re.replace_all(&sanitized, |_: &Captures| {
                rule_hits += 1;
                *marker
            }),
            RuleReplacement::UriPassword => re.replace_all(&sanitized, |caps: &Captures| {
                let secret = &caps[2];
                if is_complete_marker(secret) {
                    return caps[0].to_string();
                }
                rule_hits += 1;
                format!("{}[REDACTED_SECRET:URI_PASSWORD]{}", &caps[1], &caps[3])
            }),
            RuleReplacement::AwsSecret => re.replace_all(&sanitized, |caps: &Captures| {
                let secret = &caps[1];
                if is_complete_marker(secret) {
                    return caps[0].to_string();
                }
                rule_hits += 1;
                caps[0].replace(secret, "[REDACTED_SECRET:AWS_SECRET_KEY]")
            }),
            RuleReplacement::CredentialAssignment => {
                re.replace_all(&sanitized, |caps: &Captures| {
                    let full = &caps[0];
                    let val = caps
                        .get(1)
                        .or_else(|| caps.get(2))
                        .or_else(|| caps.get(3))
                        .map(|m| m.as_str())
                        .unwrap_or("");
                    if val.len() < 8 || is_complete_marker(val) {
                        return full.to_string();
                    }
                    rule_hits += 1;
                    if let Some(pos) = full.rfind(val) {
                        format!(
                            "{}[REDACTED_SECRET:CREDENTIAL]{}",
                            &full[..pos],
                            &full[pos + val.len()..]
                        )
                    } else {
                        full.to_string()
                    }
                })
            }
        };
        if rule_hits > 0 {
            total_redacted += rule_hits;
            sanitized = replaced.into_owned();
        }
    }

    if total_redacted > 0 {
        info!(
            "DLP Scrubber redacted {} sensitive secret(s) prior to LLM dispatch",
            total_redacted
        );
    }

    (sanitized, total_redacted)
}

/// Scrubs arbitrary freeform text (PR descriptions, comments, guidelines) and returns the sanitized string.
pub fn scrub_text(text: &str) -> String {
    scrub_diff(text).0
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
        assert!(
            scrubbed.contains(
                "postgres://admin:[REDACTED_SECRET:URI_PASSWORD]@db.example.com:5432/revue"
            )
        );
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
        assert_eq!(
            scrubbed_phrase,
            r#"+password: "[REDACTED_SECRET:CREDENTIAL]""#
        );

        let substring_diff = r#"+secret_key: "secret_key_12345678""#; // gitleaks:allow
        let (scrubbed_sub, count) = scrub_diff(substring_diff);
        assert_eq!(count, 1);
        assert_eq!(
            scrubbed_sub,
            r#"+secret_key: "[REDACTED_SECRET:CREDENTIAL]""#
        );

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

    #[test]
    fn test_scrub_multibyte_utf8_and_additional_token_families() {
        let slack = format!("xoxb-{}-{}", "123456789012", "abcdefghijklmnopqrstuvwx");
        let gitlab = format!("glpat-{}", "1234567890abcdefghij");
        let anthropic = format!("sk-ant-api03-{}", "a".repeat(32));
        let jwt = format!(
            "Bearer eyJhbGciOiJIUzI1NiJ9.{}.{}",
            "eyJzdWIiOiIxMjM0NTY3ODkwIn0", "SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"
        );
        let line = format!("+// 🔐 Tokens: {slack} | {gitlab} | {anthropic} | {jwt}\n");

        let (scrubbed, count) = scrub_diff(&line);
        assert_eq!(count, 4, "got {count}: {scrubbed}");
        assert!(scrubbed.contains("🔐 Tokens:"));
        assert!(!scrubbed.contains("abcdefghijklmnopqrstuvwx"));
        assert!(!scrubbed.contains(&gitlab));
        assert!(!scrubbed.contains(&anthropic));
        assert!(!scrubbed.contains("SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"));
    }
}
