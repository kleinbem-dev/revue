use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

fn temp_test_dir(prefix: &str) -> std::path::PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let id = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "revue-cli-test-{}-{}-{}",
        prefix,
        std::process::id(),
        id
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn cli_version_and_init_lifecycle() {
    let bin = env!("CARGO_BIN_EXE_revue");

    let ver = Command::new(bin).arg("--version").output().unwrap();
    assert!(ver.status.success());
    let stdout = String::from_utf8_lossy(&ver.stdout);
    assert!(stdout.starts_with("revue "), "unexpected version: {stdout}");

    let dir = temp_test_dir("init");
    let first = Command::new(bin)
        .arg("init")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(first.status.success());
    let cfg_file = dir.join(".revue.yaml");
    assert!(cfg_file.exists());

    std::fs::write(&cfg_file, "# sentinel\nversion: 1\n").unwrap();

    // Without --force, existing .revue.yaml must not be overwritten.
    let second = Command::new(bin)
        .arg("init")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(second.status.success());
    assert!(
        std::fs::read_to_string(&cfg_file)
            .unwrap()
            .starts_with("# sentinel")
    );

    // With --force, template overwrites sentinel.
    let forced = Command::new(bin)
        .args(["init", "--force"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(forced.status.success());
    assert!(
        !std::fs::read_to_string(&cfg_file)
            .unwrap()
            .starts_with("# sentinel")
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn mcp_stdio_protocol_and_stdout_purity() {
    let bin = env!("CARGO_BIN_EXE_revue");
    let dir = temp_test_dir("mcp");

    // Configure an unreachable local Ollama endpoint so `revue_inspect` exercises
    // the full review pipeline and emits tracing logs without touching the network.
    let cfg_path = dir.join(".revue.yaml");
    std::fs::write(
        &cfg_path,
        r#"version: 1
providers:
  cascade: ["ollama"]
  ollama_url: "http://127.0.0.1:9"
review:
  semgrep: false
"#,
    )
    .unwrap();

    let mut child = Command::new(bin)
        .args(["--config", cfg_path.to_str().unwrap(), "mcp"])
        .current_dir(&dir)
        .env("RUST_LOG", "info")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    fn send_and_recv(
        stdin: &mut std::process::ChildStdin,
        reader: &mut BufReader<std::process::ChildStdout>,
        msg: &str,
    ) -> Value {
        writeln!(stdin, "{msg}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        let n = reader.read_line(&mut line).unwrap();
        assert!(n > 0, "unexpected EOF from MCP server");
        serde_json::from_str(&line)
            .unwrap_or_else(|e| panic!("stdout contaminated by non-JSON line {line:?}: {e}"))
    }

    // 1. initialize
    let init_resp = send_and_recv(
        &mut stdin,
        &mut reader,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    );
    assert_eq!(init_resp["id"], 1);
    assert_eq!(init_resp["result"]["serverInfo"]["name"], "revue");

    // 2. Notification produces no response; follow immediately with ping.
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#
    )
    .unwrap();
    let ping_resp = send_and_recv(
        &mut stdin,
        &mut reader,
        r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
    );
    assert_eq!(ping_resp["id"], 2);

    // 3. Malformed JSON -> -32700 parse error without crashing loop.
    let parse_err = send_and_recv(&mut stdin, &mut reader, "{not valid json");
    assert_eq!(parse_err["error"]["code"], -32700);

    // 4. Unknown method -> -32601 method not found.
    let method_err = send_and_recv(
        &mut stdin,
        &mut reader,
        r#"{"jsonrpc":"2.0","id":3,"method":"does/not_exist"}"#,
    );
    assert_eq!(method_err["id"], 3);
    assert_eq!(method_err["error"]["code"], -32601);

    // 5. Empty diff -> structured tool error.
    let empty_inspect = send_and_recv(
        &mut stdin,
        &mut reader,
        &json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": { "name": "revue_inspect", "arguments": { "diff": "   " } }
        })
        .to_string(),
    );
    assert_eq!(empty_inspect["result"]["isError"], true);

    // 6. Real diff triggers tracing `info!`/`warn!` logs; stdout must still be pure JSON-RPC.
    let diff =
        "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1,2 @@\n pub fn a() {}\n+pub fn b() {}\n";
    let real_inspect = send_and_recv(
        &mut stdin,
        &mut reader,
        &json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": { "name": "revue_inspect", "arguments": { "diff": diff } }
        })
        .to_string(),
    );
    assert_eq!(real_inspect["id"], 5);
    assert!(real_inspect["result"]["content"].is_array());

    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let _ = std::fs::remove_dir_all(dir);
}
