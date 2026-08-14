use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use rzn_tools_core::transport::DEFAULT_MAX_FRAME_BYTES;
use rzn_tools_mcp::SIDECAR_PROTOCOL_VERSION;
use serde_json::{json, Value};

struct Sidecar {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Sidecar {
    fn spawn(connectors: &str, ambient_env: Option<(&str, &str)>) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rzn-tools-mcp"));
        command
            .arg("sidecar")
            .arg("--connectors")
            .arg(connectors)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some((name, value)) = ambient_env {
            command.env(name, value);
        }
        let mut child = command.spawn().expect("spawn rzn-tools sidecar");
        let stdin = child.stdin.take().expect("sidecar stdin");
        let stdout = BufReader::new(child.stdout.take().expect("sidecar stdout"));
        Self {
            child,
            stdin,
            stdout,
        }
    }

    fn request(&mut self, request: Value) -> Value {
        writeln!(self.stdin, "{request}").expect("write MCP request");
        self.stdin.flush().expect("flush MCP request");
        let mut line = String::new();
        self.stdout.read_line(&mut line).expect("read MCP response");
        assert!(!line.is_empty(), "sidecar closed stdout unexpectedly");
        serde_json::from_str(&line).expect("JSON MCP response")
    }

    fn raw_request(&mut self, request: &[u8]) -> Value {
        self.stdin
            .write_all(request)
            .expect("write raw MCP request");
        self.stdin.flush().expect("flush raw MCP request");
        let mut line = String::new();
        self.stdout
            .read_line(&mut line)
            .expect("read raw MCP response");
        serde_json::from_str(&line).expect("JSON raw MCP response")
    }

    fn close(mut self) {
        drop(self.stdin);
        assert!(self.child.wait().expect("wait sidecar").success());
    }
}

fn initialize() -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": "init",
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "sidecar-test", "version": "1"}
        }
    })
}

#[test]
fn sidecar_handshake_tool_call_and_eof_are_stable() {
    let mut sidecar = Sidecar::spawn("openai-search", None);
    let init = sidecar.request(initialize());
    let result = init.get("result").expect("initialize result");
    assert_eq!(result["serverInfo"]["name"], "rzn-tools");
    assert_eq!(result["protocolVersion"], SIDECAR_PROTOCOL_VERSION);
    assert!(result["capabilities"]["tools"].is_object());

    let set = sidecar.request(json!({
        "jsonrpc": "2.0",
        "id": "set",
        "method": "secrets/set",
        "params": {"provider": "openai-search", "secrets": {"api_key": "invocation-key"}}
    }));
    assert_eq!(set["result"]["ok"], true);

    let test = sidecar.request(json!({
        "jsonrpc": "2.0",
        "id": "call",
        "method": "tools/call",
        "params": {"name": "auth/openai-search/test", "arguments": {}}
    }));
    assert_eq!(test["result"]["structuredContent"]["ok"], true);
    sidecar.close();
}

#[test]
fn sidecar_rejects_malformed_and_oversize_frames() {
    let mut sidecar = Sidecar::spawn("hackernews", None);
    let malformed = sidecar.raw_request(b"{\n");
    assert_eq!(malformed["error"]["code"], -32700);

    let mut oversize = vec![b'x'; DEFAULT_MAX_FRAME_BYTES + 1];
    oversize.push(b'\n');
    let refused = sidecar.raw_request(&oversize);
    assert_eq!(refused["error"]["code"], -32600);

    let init = sidecar.request(initialize());
    assert!(init.get("result").is_some());
    sidecar.close();
}

#[test]
fn sidecar_does_not_inherit_tenant_credentials() {
    let mut sidecar = Sidecar::spawn("openai-search", Some(("OPENAI_API_KEY", "tenant-a-canary")));
    let _ = sidecar.request(initialize());
    let response = sidecar.request(json!({
        "jsonrpc": "2.0",
        "id": "ambient",
        "method": "tools/call",
        "params": {"name": "auth/openai-search/test", "arguments": {}}
    }));
    assert!(response["error"].is_object());
    assert!(!response.to_string().contains("tenant-a-canary"));
    sidecar.close();
}

#[test]
fn sidecar_exits_cleanly_on_eof() {
    let sidecar = Sidecar::spawn("hackernews", None);
    sidecar.close();
}

#[test]
fn invalid_sidecar_invocation_exits_without_starting_http() {
    let output = Command::new(env!("CARGO_BIN_EXE_rzn-tools-mcp"))
        .args(["sidecar", "--transport", "http"])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .output()
        .expect("run invalid sidecar invocation");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("sidecar mode only supports stdio"));
}
