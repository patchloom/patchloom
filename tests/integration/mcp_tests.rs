use super::*;

#[test]
fn test_mcp_setup_documents_search_files_modes() {
    let doc = fs::read_to_string(repo_root().join("docs/getting-started/mcp-setup.md")).unwrap();
    assert!(doc.contains("literal, case-insensitive, count, file-only, multiline, invert-match, and assert-count modes"));
}

#[test]
fn test_mcp_setup_documents_text_file_skip_semantics() {
    let doc = fs::read_to_string(repo_root().join("docs/getting-started/mcp-setup.md")).unwrap();
    assert!(doc.contains("Binary and invalid UTF-8 files are skipped"));
}

#[cfg(feature = "mcp-http")]
#[test]
fn test_mcp_http_port_requires_http_flag() {
    if !has_mcp_http_support() {
        return;
    }
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["mcp-server", "--port", "3000"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--http"));
}

#[cfg(feature = "mcp-http")]
#[test]
fn test_mcp_http_host_requires_http_flag() {
    if !has_mcp_http_support() {
        return;
    }
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["mcp-server", "--host", "0.0.0.0"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--http"));
}

#[cfg(feature = "mcp-http")]
#[test]
fn test_mcp_http_tls_cert_requires_tls_key() {
    if !has_mcp_http_support() {
        return;
    }
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["mcp-server", "--http", "--tls-cert", "cert.pem"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--tls-key"));
}

#[cfg(feature = "mcp-http")]
#[test]
fn test_mcp_http_tls_key_requires_tls_cert() {
    if !has_mcp_http_support() {
        return;
    }
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["mcp-server", "--http", "--tls-key", "key.pem"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--tls-cert"));
}

/// Verify that invalid TLS cert content produces a clear error.
#[cfg(feature = "mcp-http")]
#[test]
fn test_mcp_http_invalid_tls_cert_fails_with_error() {
    if !has_mcp_http_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("bad-cert.pem"), "not a certificate\n").unwrap();
    fs::write(dir.path().join("bad-key.pem"), "not a key\n").unwrap();
    Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "mcp-server",
            "--http",
            "--tls-cert",
            dir.path().join("bad-cert.pem").to_str().unwrap(),
            "--tls-key",
            dir.path().join("bad-key.pem").to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("TLS"));
}

/// Verify that search_files works over HTTPS (TLS) transport with --port 0.
/// This exercises the TLS server setup (axum_server::bind_rustls) and the
/// ephemeral port banner fix (#867).
#[cfg(feature = "mcp-http")]
#[tokio::test]
async fn test_mcp_https_search_files_round_trip() {
    if !has_mcp_http_support() {
        return;
    }

    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("hello.txt"), "hello tls world\n").unwrap();

    // Generate a self-signed certificate with rcgen.
    let ca = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()])
        .expect("self-signed cert");

    let cert_pem = ca.cert.pem();
    let key_pem = ca.signing_key.serialize_pem();

    let cert_path = dir.path().join("cert.pem");
    let key_path = dir.path().join("key.pem");
    fs::write(&cert_path, &cert_pem).unwrap();
    fs::write(&key_path, &key_pem).unwrap();

    let bin = assert_cmd::cargo::cargo_bin("patchloom");
    let mut child = tokio::process::Command::new(&bin)
        .args([
            "mcp-server",
            "--http",
            "--port",
            "0",
            "--tls-cert",
            cert_path.to_str().unwrap(),
            "--tls-key",
            key_path.to_str().unwrap(),
        ])
        .current_dir(dir.path())
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn mcp-server --http --tls-*");

    // Read stderr to discover the actual bound port.
    let stderr = child.stderr.take().unwrap();
    let mut reader = tokio::io::BufReader::new(stderr);
    let mut line = String::new();
    tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
        .await
        .expect("failed to read HTTPS server banner");

    // Parse "MCP HTTPS server listening on https://127.0.0.1:PORT/mcp"
    let url = line.trim().rsplit("on ").next().expect("no URL in banner");
    assert!(
        url.starts_with("https://"),
        "expected https:// URL in banner: {line}"
    );
    // Verify the banner shows a real port, not 0.
    assert!(
        !url.contains(":0/"),
        "banner should show real ephemeral port, not :0: {url}"
    );

    // Build a reqwest client that trusts the self-signed CA.
    let ca_cert = reqwest::tls::Certificate::from_pem(cert_pem.as_bytes()).expect("parse CA cert");
    let http_client = reqwest::Client::builder()
        .pool_max_idle_per_host(0)
        .add_root_certificate(ca_cert)
        .build()
        .expect("build reqwest client");

    // Connect an MCP client over Streamable HTTPS.
    use rmcp::ServiceExt;
    let config =
        rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(url);
    let transport =
        rmcp::transport::StreamableHttpClientTransport::with_client(http_client, config);
    let client: rmcp::service::RunningService<rmcp::RoleClient, ()> =
        ().serve(transport).await.expect("HTTPS client connect");

    // List tools.
    let tools = client.peer().list_all_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    assert!(
        names.contains(&"search_files"),
        "search_files tool should be listed over HTTPS"
    );

    // Call search_files.
    let params = rmcp::model::CallToolRequestParams::new("search_files".to_string())
        .with_arguments(
            serde_json::from_value(serde_json::json!({"pattern": "tls", "paths": ["."]})).unwrap(),
        );
    let result = client.peer().call_tool(params).await.unwrap();
    assert!(
        !result.is_error.unwrap_or(false),
        "search_files should succeed over HTTPS"
    );
    let text = result
        .content
        .first()
        .and_then(|c| match c {
            rmcp::model::ContentBlock::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .unwrap_or_default();
    assert!(
        text.contains("hello tls world"),
        "search result should contain match: {text}"
    );

    client.cancel().await.unwrap();
    child.kill().await.ok();
}
