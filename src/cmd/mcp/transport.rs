//! MCP transport layer: ServerHandler implementation and server startup.

use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, ErrorData as McpError, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ServerHandler, ServiceExt};

use crate::cli::global::GlobalFlags;

use super::PatchloomService;
use super::surface::McpSurface;

/// Server instructions for agents.
///
/// Must match the active [`McpSurface`]: core mode must not advertise tools
/// that were not registered at handshake (#1994 honesty / review follow-up).
/// AST category is also omitted when the `ast` feature is disabled (full only).
pub(super) fn server_instructions(surface: McpSurface) -> String {
    match surface {
        McpSurface::Core => core_server_instructions(),
        McpSurface::Full => full_server_instructions(),
    }
}

/// Instructions when `PATCHLOOM_MCP_SURFACE=core` (exactly [`super::surface::CORE_MCP_TOOL_NAMES`]).
fn core_server_instructions() -> String {
    let mut s = String::from(
        "This server is running with PATCHLOOM_MCP_SURFACE=core (minimal tool pack). \
         Only the tools below are registered; do not call others. Restart with \
         PATCHLOOM_MCP_SURFACE=full (or unset) for the full inventory.\n\n\
         Prefer 'execute_plan' for multi-op or multi-file work (atomicity). \
         Per-call success does not guarantee combined success if you issue \
         conflicting parallel writes.\n\n\
         Explore with list_files/search_files/read_file (prefer relative paths); shell \
         cat/find/sed only for build/test unless the user overrides. Do not also install \
         a generic filesystem MCP for list+edit. MCP allows absolute paths only when they \
         resolve inside the workspace; `../` and outside paths are rejected.\n\n\
         Core tools:\n\
         - read_file, search_files, list_files: inspect, find, and inventory files\n\
         - replace_text, batch_replace: literal/regex text edits\n\
         - doc_get, doc_set, doc_query: parser-backed JSON/YAML/TOML by selector path\n\
         - md_replace_section: replace a markdown heading section\n\
         - execute_plan: multi-op atomic plans (tx)\n\
         - server_info: cwd, surface, tool_count, version, protocol_version\n\n\
         Use doc_get/doc_set/doc_query for structured config; replace_text only where structure does not matter.\n\n",
    );
    // Shared packaging blocks (#2070); core name map omits unregistered ast_* tools.
    s.push_str(crate::cmd::agent_packaging::canonical_name_map_markdown_core());
    s.push_str(crate::cmd::agent_packaging::explore_guidance_markdown());
    s.push_str(crate::cmd::agent_packaging::yaml_style_honesty_markdown());
    s
}

/// Full-inventory instructions; AST category omitted when `ast` is disabled.
fn full_server_instructions() -> String {
    let mut s = String::from(
        "Use these tools for ALL file operations (edits and explore). Prefer \
         list_files/search_files/read_file over shell cat/find/ls/sed (and over a second \
         filesystem MCP) when MCP is connected; shell for build/test/run unless the user \
         overrides. Prefer 'execute_plan' (or tx plans) \
         for any multi-op or multi-file work to ensure atomicity and avoid races from \
         parallel calls on the same paths. Use batch_replace/batch_tidy only for uniform \
         ops across files. Per-call success does not guarantee combined success if you \
         issue conflicting parallel writes.\n\n\
         Coding agents with tight context: set PATCHLOOM_MCP_SURFACE=core (product default \
         remains full for compatibility).\n\n\
         Tool categories:\n\
         - Document ops (JSON/YAML/TOML by selector path): doc_set, doc_get, doc_delete, \
         doc_merge, doc_query, doc_update, doc_ensure, doc_move, doc_append, doc_prepend, \
         doc_delete_where, doc_diff\n\
         - Markdown ops (by heading): md_replace_section, md_upsert_bullet, \
         md_table_append, md_insert_after_heading, md_insert_after_section, md_insert_before_heading, \
         md_move_section, md_dedupe_headings, md_lint\n\
         - Text ops: replace_text, batch_replace, search_files, list_files, apply_fragment, apply_patch, tidy_check\n\
         - File ops: create_file, read_file, delete_file, move_file, append_file, \
         prepend_file, fix_whitespace, batch_tidy, git_status, undo_list, undo_restore\n",
    );
    // Continuation lines after `\` discard leading whitespace. Start each
    // push_str body on the category marker so we do not inject indent spaces.
    #[cfg(feature = "ast")]
    s.push_str(
        "- AST ops (code-aware, 20 languages): ast_list, ast_read, ast_rename, \
         ast_replace, ast_replace_symbol, ast_delete_symbol, ast_rewrite_signature, ast_search, ast_refs, ast_impact, ast_deps, ast_diff, ast_imports, \
         ast_insert, ast_wrap, ast_move, ast_reorder, ast_group, ast_extract_to_file, \
         ast_split, ast_map, ast_validate\n",
    );
    s.push_str(
        "- Plan ops: execute_plan, explain_plan\n\
         - Server: server_info\n\n\
         Use doc_* tools for parser-backed JSON/YAML/TOML mutations by selector path \
         (e.g. doc_set for setting values, doc_merge for merging objects). Use replace_text \
         only for literal or regex text replacement where structure does not matter.\n\n",
    );
    s.push_str(crate::cmd::agent_packaging::canonical_name_map_markdown());
    s.push_str(crate::cmd::agent_packaging::explore_guidance_markdown());
    s.push_str(crate::cmd::agent_packaging::yaml_style_honesty_markdown());
    s
}

impl ServerHandler for PatchloomService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(server_instructions(self.surface()))
            .with_server_info(Implementation::new("patchloom", env!("CARGO_PKG_VERSION")))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        // Use Default for SEP-2549/SEP-2322 optional fields (result_type, ttl_ms,
        // cache_scope) so we stay compatible when rmcp adds more result metadata.
        Ok(ListToolsResult {
            tools: self.tool_router.list_all(),
            ..ListToolsResult::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let tool_name = request.name.clone();
        crate::verbose!("mcp: tool call -> {tool_name}");
        let start = std::time::Instant::now();
        let tc = ToolCallContext::new(self, request, context);
        let result = self.tool_router.call(tc).await;
        let duration_ms = start.elapsed().as_millis() as u64;
        crate::verbose!(
            "mcp: {tool_name} completed in {duration_ms}ms (ok={})",
            result.is_ok()
        );
        self.log_tool_call(&tool_name, duration_ms, &result).await;
        result
    }
}

/// True when `--host` is a loopback bind (`127.0.0.0/8`, `::1`, `localhost`).
///
/// Unspecified addresses (`0.0.0.0`, `::`) are not loopback. Used to fail
/// closed on unauthenticated Streamable HTTP binds.
#[cfg(feature = "mcp-http")]
pub(crate) fn is_loopback_http_bind_host(host: &str) -> bool {
    let host = host.trim();
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let host = host
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(host);
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_loopback(),
        Ok(std::net::IpAddr::V6(ip)) => {
            ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
        Err(_) => false,
    }
}

/// Refuse unauthenticated Streamable HTTP on a non-loopback bind.
///
/// Streamable HTTP has no token. Binding `0.0.0.0` or another non-loopback
/// address requires an explicit `--allow-unauthenticated` opt-in.
#[cfg(feature = "mcp-http")]
pub(crate) fn check_unauthenticated_http_bind(
    host: &str,
    allow_unauthenticated: bool,
) -> Result<(), crate::exit::InvalidInputError> {
    if allow_unauthenticated || is_loopback_http_bind_host(host) {
        return Ok(());
    }
    Err(crate::exit::InvalidInputError {
        msg: format!(
            "refusing unauthenticated HTTP bind on non-loopback address '{host}'. \
             Streamable HTTP has no authentication. Bind 127.0.0.1, ::1, or localhost, \
             or pass --allow-unauthenticated to opt in."
        ),
    })
}

/// Host names accepted in the HTTP `Host` header for Streamable HTTP.
///
/// Always includes rmcp defaults (`localhost`, `127.0.0.1`, `::1`). A
/// specific bind IP (not unspecified `0.0.0.0` / `::`, and not a hostname
/// that is only localhost) is also included so clients that send that
/// address are accepted. Extra names come from `--allowed-host` (trimmed;
/// empty skipped; case-insensitive dedup). The result is never empty
/// (empty would accept any Host).
#[cfg(feature = "mcp-http")]
pub(crate) fn http_allowed_hosts(bind_host: &str, extra: &[String]) -> Vec<String> {
    let mut hosts = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ];
    if let Some(bind) = bind_host_as_allowed(bind_host) {
        push_unique_host(&mut hosts, &bind);
    }
    for name in extra {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            push_unique_host(&mut hosts, trimmed);
        }
    }
    hosts
}

/// Bind host to add to the Host allowlist, if any.
///
/// Unspecified addresses (`0.0.0.0`, `::`) are not client Host values.
/// `localhost` is already in the rmcp defaults.
#[cfg(feature = "mcp-http")]
fn bind_host_as_allowed(bind_host: &str) -> Option<String> {
    let host = bind_host.trim();
    if host.is_empty() || host.eq_ignore_ascii_case("localhost") {
        return None;
    }
    let stripped = host
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(host);
    match stripped.parse::<std::net::IpAddr>() {
        Ok(ip) if ip_is_unspecified(ip) => None,
        Ok(_) => Some(stripped.to_string()),
        Err(_) => Some(host.to_string()),
    }
}

#[cfg(feature = "mcp-http")]
fn ip_is_unspecified(ip: std::net::IpAddr) -> bool {
    ip.is_unspecified()
        || match ip {
            std::net::IpAddr::V6(v6) => v6.to_ipv4_mapped().is_some_and(|v4| v4.is_unspecified()),
            std::net::IpAddr::V4(_) => false,
        }
}

#[cfg(feature = "mcp-http")]
fn push_unique_host(hosts: &mut Vec<String>, candidate: &str) {
    if !hosts.iter().any(|h| h.eq_ignore_ascii_case(candidate)) {
        hosts.push(candidate.to_string());
    }
}

/// Typed bind/listen failure so `--json` keeps `error_kind: invalid_input`.
#[cfg(feature = "mcp-http")]
fn bind_listen_error(addr: impl std::fmt::Display, err: impl std::fmt::Display) -> anyhow::Error {
    anyhow::Error::new(crate::exit::InvalidInputError {
        msg: format!("failed to bind {addr}: {err}"),
    })
}

/// Parse `--host` + `--port` into a bind address.
///
/// Accepts hostnames (`localhost`), bare IPv6 (`::1`), bracketed IPv6
/// (`[::1]`), and dotted IPv4. `SocketAddr` parse alone rejects the first two.
#[cfg(feature = "mcp-http")]
pub(crate) fn parse_http_bind_addr(
    host: &str,
    port: u16,
) -> Result<std::net::SocketAddr, crate::exit::InvalidInputError> {
    use std::net::{IpAddr, SocketAddr, ToSocketAddrs};

    let host = host.trim();
    let host = host
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(host);
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, port));
    }
    (host, port)
        .to_socket_addrs()
        .map_err(|e| crate::exit::InvalidInputError {
            msg: format!("invalid bind address: {e}"),
        })?
        .next()
        .ok_or_else(|| crate::exit::InvalidInputError {
            msg: format!("invalid bind address: no addresses for {host}:{port}"),
        })
}

/// Streamable HTTP listen options for [`run_mcp_http_server`].
#[cfg(feature = "mcp-http")]
pub(crate) struct McpHttpListen<'a> {
    pub host: &'a str,
    pub port: u16,
    pub tls_cert: Option<&'a std::path::Path>,
    pub tls_key: Option<&'a std::path::Path>,
    pub allow_unauthenticated: bool,
    pub allowed_hosts: &'a [String],
}

/// Run the MCP server over Streamable HTTP (optionally with TLS).
#[cfg(feature = "mcp-http")]
pub(crate) fn run_mcp_http_server(
    global: &GlobalFlags,
    log: Option<String>,
    listen: McpHttpListen<'_>,
) -> anyhow::Result<u8> {
    use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
    use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
    use tokio_util::sync::CancellationToken;

    check_unauthenticated_http_bind(listen.host, listen.allow_unauthenticated)
        .map_err(anyhow::Error::new)?;

    let cwd = global.resolve_cwd()?;
    let ct = CancellationToken::new();

    let config = StreamableHttpServerConfig::default()
        .with_cancellation_token(ct.child_token())
        .with_allowed_hosts(http_allowed_hosts(listen.host, listen.allowed_hosts));

    let log_path = log;
    let io_gate = std::sync::Arc::new(std::sync::RwLock::new(()));
    let service = StreamableHttpService::new(
        move || {
            PatchloomService::new_sharing_gate(
                cwd.clone(),
                log_path.clone(),
                std::sync::Arc::clone(&io_gate),
            )
            .map_err(std::io::Error::other)
        },
        std::sync::Arc::new(LocalSessionManager::default()),
        config,
    );

    let app = axum::Router::new().nest_service("/mcp", service);
    let addr = parse_http_bind_addr(listen.host, listen.port).map_err(anyhow::Error::new)?;
    let show_banner = !global.quiet && !global.json && !global.jsonl;

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        if let (Some(cert), Some(key)) = (listen.tls_cert, listen.tls_key) {
            let tls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key)
                .await
                .map_err(|e| {
                    anyhow::Error::new(crate::exit::InvalidInputError {
                        msg: format!("TLS config error: {e}"),
                    })
                })?;

            let handle = axum_server::Handle::new();
            let h = handle.clone();
            let ct2 = ct.clone();
            tokio::spawn(async move {
                tokio::signal::ctrl_c().await.ok();
                ct2.cancel();
                h.graceful_shutdown(Some(std::time::Duration::from_secs(5)));
            });

            // Print the banner once the server is actually bound so that
            // --port 0 shows the real ephemeral port (fixes #867).
            let h_addr = handle.clone();
            tokio::spawn(async move {
                if let Some(real_addr) = h_addr.listening().await
                    && show_banner
                {
                    eprintln!("MCP HTTPS server listening on https://{real_addr}/mcp");
                }
            });

            axum_server::bind_rustls(addr, tls_config)
                .handle(handle)
                .serve(app.into_make_service())
                .await
                .map_err(|e| bind_listen_error(addr, e))?;
        } else {
            let ct2 = ct.clone();
            tokio::spawn(async move {
                tokio::signal::ctrl_c().await.ok();
                ct2.cancel();
            });

            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .map_err(|e| bind_listen_error(addr, e))?;
            if show_banner {
                eprintln!(
                    "MCP HTTP server listening on http://{}/mcp",
                    listener.local_addr()?
                );
            }

            axum::serve(listener, app)
                .with_graceful_shutdown(ct.cancelled_owned())
                .await
                .map_err(|e| anyhow::anyhow!("HTTP server error: {e}"))?;
        }
        Ok::<(), anyhow::Error>(())
    })?;

    Ok(crate::exit::SUCCESS)
}

/// Run the MCP server on stdio.
pub(crate) fn run_mcp_server(global: &GlobalFlags, log: Option<String>) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    let service = PatchloomService::new(cwd, log)?;

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let server = service
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|e| anyhow::anyhow!("MCP server error: {e}"))?;
        server
            .waiting()
            .await
            .map_err(|e| anyhow::anyhow!("MCP server error: {e}"))?;
        Ok::<(), anyhow::Error>(())
    })?;

    Ok(crate::exit::SUCCESS)
}

#[cfg(all(test, feature = "mcp-http"))]
mod bind_host_tests {
    use super::{
        bind_listen_error, check_unauthenticated_http_bind, http_allowed_hosts,
        is_loopback_http_bind_host, parse_http_bind_addr,
    };

    fn default_http_hosts() -> Vec<String> {
        vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
            "::1".to_string(),
        ]
    }

    #[test]
    fn http_allowed_hosts_unspecified_bind_is_defaults_only() {
        let hosts = http_allowed_hosts("0.0.0.0", &[]);
        assert!(!hosts.is_empty(), "empty allowlist accepts any Host header");
        assert_eq!(hosts, default_http_hosts());
        assert!(!hosts.iter().any(|h| h == "0.0.0.0"));
    }

    #[test]
    fn http_allowed_hosts_specific_ip_includes_bind() {
        let hosts = http_allowed_hosts("192.168.0.10", &[]);
        for expected in ["localhost", "127.0.0.1", "::1", "192.168.0.10"] {
            assert!(
                hosts.iter().any(|h| h == expected),
                "missing {expected} in {hosts:?}"
            );
        }
    }

    #[test]
    fn http_allowed_hosts_unspecified_v6_plus_extra() {
        let extra = ["mcp.example".to_string()];
        let hosts = http_allowed_hosts("::", &extra);
        for expected in ["localhost", "127.0.0.1", "::1", "mcp.example"] {
            assert!(
                hosts.iter().any(|h| h == expected),
                "missing {expected} in {hosts:?}"
            );
        }
        assert!(
            !hosts.iter().any(|h| h == "::" || h == "0.0.0.0"),
            "unspecified bind must not be a client Host: {hosts:?}"
        );
    }

    #[test]
    fn http_allowed_hosts_trims_skips_empty_and_dedups() {
        let extra = [
            "  mcp.example  ".to_string(),
            String::new(),
            "   ".to_string(),
            "localhost".to_string(),
            "LOCALHOST".to_string(),
            "MCP.example".to_string(),
        ];
        let hosts = http_allowed_hosts("127.0.0.1", &extra);
        assert!(!hosts.is_empty());
        assert_eq!(
            hosts
                .iter()
                .filter(|h| h.eq_ignore_ascii_case("localhost"))
                .count(),
            1
        );
        assert_eq!(
            hosts
                .iter()
                .filter(|h| h.eq_ignore_ascii_case("mcp.example"))
                .count(),
            1
        );
        assert!(hosts.iter().any(|h| h == "mcp.example"));
        assert!(!hosts.iter().any(|h| h.trim().is_empty()));
    }

    #[test]
    fn http_allowed_hosts_strips_ipv6_brackets() {
        let hosts = http_allowed_hosts("[2001:db8::10]", &[]);
        assert!(
            hosts.iter().any(|h| h == "2001:db8::10"),
            "bracketed IPv6 bind should be stored without brackets: {hosts:?}"
        );
        assert!(!hosts.iter().any(|h| h == "[2001:db8::10]"));
    }

    #[test]
    fn run_mcp_http_server_does_not_disable_allowed_hosts() {
        let src = include_str!("transport.rs");
        let start = src
            .find("pub(crate) fn run_mcp_http_server")
            .expect("run_mcp_http_server");
        let rest = &src[start..];
        let end = rest
            .find("pub(crate) fn run_mcp_server")
            .unwrap_or(rest.len());
        let body = &rest[..end];
        assert!(
            body.contains("new_sharing_gate"),
            "HTTP sessions must share one write lock (#2610)"
        );
        assert!(
            !body.contains("disable_allowed_hosts"),
            "run_mcp_http_server must not call disable_allowed_hosts"
        );
        assert!(
            body.contains("with_allowed_hosts(http_allowed_hosts"),
            "run_mcp_http_server must set allowed hosts via http_allowed_hosts"
        );
        assert!(
            body.contains("bind_listen_error"),
            "HTTP and HTTPS listen failures must use bind_listen_error"
        );
    }

    #[test]
    fn bind_listen_error_is_invalid_input() {
        let err = bind_listen_error("127.0.0.1:1", "address in use");
        let typed = err
            .downcast_ref::<crate::exit::InvalidInputError>()
            .expect("typed");
        assert!(
            typed.msg.contains("failed to bind 127.0.0.1:1"),
            "{}",
            typed.msg
        );
        assert!(typed.msg.contains("address in use"), "{}", typed.msg);
    }

    #[test]
    fn parse_http_bind_addr_accepts_localhost() {
        let addr = parse_http_bind_addr("localhost", 8377).expect("localhost");
        assert!(
            addr.ip().is_loopback(),
            "localhost must resolve to loopback"
        );
        assert_eq!(addr.port(), 8377);
    }

    #[test]
    fn parse_http_bind_addr_accepts_bare_ipv6() {
        let addr = parse_http_bind_addr("::1", 8377).expect("::1");
        assert_eq!(
            addr,
            std::net::SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, 8377))
        );
    }

    #[test]
    fn parse_http_bind_addr_accepts_bracketed_ipv6() {
        let addr = parse_http_bind_addr("[::1]", 8377).expect("[::1]");
        assert_eq!(
            addr,
            std::net::SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, 8377))
        );
    }

    #[test]
    fn parse_http_bind_addr_accepts_ipv4() {
        let addr = parse_http_bind_addr("127.0.0.1", 8377).expect("127.0.0.1");
        assert_eq!(
            addr,
            std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 8377))
        );
    }

    #[test]
    fn loopback_hosts_are_loopback() {
        for host in [
            "127.0.0.1",
            "127.0.0.2",
            "127.255.255.255",
            "::1",
            "[::1]",
            "localhost",
            "LOCALHOST",
            " ::ffff:127.0.0.1 ",
        ] {
            assert!(
                is_loopback_http_bind_host(host),
                "expected loopback: {host:?}"
            );
        }
    }

    #[test]
    fn non_loopback_hosts_are_not_loopback() {
        for host in [
            "0.0.0.0",
            "::",
            "[::]",
            "192.168.1.1",
            "10.0.0.1",
            "8.8.8.8",
            "example.com",
            "",
        ] {
            assert!(
                !is_loopback_http_bind_host(host),
                "expected non-loopback: {host:?}"
            );
        }
    }

    #[test]
    fn non_loopback_refused_without_allow_flag() {
        let err = check_unauthenticated_http_bind("0.0.0.0", false).unwrap_err();
        assert!(
            err.msg.contains("no authentication"),
            "message must say HTTP has no auth: {}",
            err.msg
        );
        assert!(
            err.msg.contains("--allow-unauthenticated"),
            "message must name the opt-in flag: {}",
            err.msg
        );
        let wrapped = anyhow::Error::new(err);
        assert_eq!(
            crate::fallback::error_kind_str(&wrapped),
            Some("invalid_input")
        );
    }

    #[test]
    fn loopback_ok_without_allow_flag() {
        check_unauthenticated_http_bind("127.0.0.1", false).unwrap();
        check_unauthenticated_http_bind("localhost", false).unwrap();
        check_unauthenticated_http_bind("::1", false).unwrap();
    }

    #[test]
    fn non_loopback_ok_with_allow_flag() {
        check_unauthenticated_http_bind("0.0.0.0", true).unwrap();
        check_unauthenticated_http_bind("192.168.0.10", true).unwrap();
    }
}
