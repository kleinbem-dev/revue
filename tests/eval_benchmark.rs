use revue::diff::ParsedDiff;
use serde::Deserialize;
use std::fs;
use std::path::Path;

/// Labelled ground truth review evaluation fixture.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct EvalFixture {
    id: String,
    name: String,
    description: String,
    diff: String,
    expected_type: String,
    #[serde(default)]
    expected_file: String,
    #[serde(default)]
    expected_category: Option<String>,
    #[serde(default)]
    expected_min_severity: Option<String>,
    #[serde(default)]
    expected_dismissal: bool,
    #[serde(default)]
    intent: Option<revue::forges::PrMetadata>,
}

#[test]
fn test_eval_benchmark_fixtures_load_and_parse() {
    let fixtures_dir = Path::new("tests/fixtures/eval");
    assert!(fixtures_dir.exists(), "eval fixtures directory must exist");

    let entries = fs::read_dir(fixtures_dir).expect("failed to read fixtures directory");
    let mut count = 0;

    for entry in entries {
        let entry = entry.expect("valid directory entry");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let content = fs::read_to_string(&path).expect("readable fixture file");
            let fixture: EvalFixture =
                serde_json::from_str(&content).expect("valid fixture JSON schema");

            assert!(!fixture.id.is_empty());
            assert!(!fixture.name.is_empty());
            assert!(!fixture.diff.is_empty());

            // Validate that ParsedDiff correctly parses the fixture diff
            let parsed = ParsedDiff::parse(&fixture.diff).expect("diff must parse cleanly");
            assert!(
                !parsed.files.is_empty(),
                "parsed files should not be empty for {}",
                fixture.id
            );

            if !fixture.expected_file.is_empty() {
                assert!(
                    parsed.files.iter().any(|f| f.new_path == fixture.expected_file),
                    "fixture {} expected file {} in diff",
                    fixture.id,
                    fixture.expected_file
                );
            }

            count += 1;
        }
    }

    assert!(count >= 3, "expected at least 3 benchmark fixtures, found {}", count);
}

#[test]
fn test_benchmark_detective_prompt_eval() {
    let content = fs::read_to_string("tests/fixtures/eval/03_silent_error_drop.json")
        .expect("readable fixture");
    let fixture: EvalFixture = serde_json::from_str(&content).unwrap();

    let diff = ParsedDiff::parse(&fixture.diff).unwrap();
    let config = revue::config::RevueConfig::default();

    let prompt = revue::reviewer::generate_pass1_detective_prompt(
        &diff,
        &config,
        &[],
        fixture.intent.as_ref(),
    );

    // Verify intent is injected into Pass 1 prompt with exact rendered lines
    assert!(prompt.contains("PULL REQUEST INTENT & CONTEXT:"));
    if let Some(ref intent) = fixture.intent {
        assert!(prompt.contains(&format!("- Title: {}", intent.title)));
        if !intent.author.is_empty() {
            assert!(prompt.contains(&format!("- Author: {}", intent.author)));
        }
        if !intent.description.is_empty() {
            assert!(prompt.contains(&intent.description));
        }
    }
    assert!(prompt.contains(&format!("=== MODIFIED FILE: {} ===", fixture.expected_file)));
}
