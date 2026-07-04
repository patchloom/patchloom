//! MCP transport layer: ServerHandler implementation and server startup.

use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ErrorData as McpError, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerInfo,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ServerHandler, ServiceExt};

use crate::cli::global::GlobalFlags;

use super::PatchloomService;

/// Server instructions for agents; AST category omitted when `ast` is disabled.
fn server_instructions() -> String {
    let mut s = String::from(
        "Use these tools for ALL file operations. Prefer 'execute_plan' (or tx plans) \
         for any multi-op or multi-file work to ensure atomicity and avoid races from \
         parallel calls on the same paths. Use batch_replace/batch_tidy only for uniform \
         ops across files. Per-call success does not guarantee combined success if you \
         issue conflicting parallel writes.\n\n\
         Tool categories:\n\
         - Document ops (JSON/YAML/TOML by selector path): doc_set, doc_get, doc_delete, \
         doc_merge, doc_query, doc_update, doc_ensure, doc_move, doc_append, doc_prepend, \
         doc_delete_where, doc_diff\n\
         - Markdown ops (by heading): md_replace_section, md_upsert_bullet, \
         md_table_append, md_insert_after_heading, md_insert_before_heading, \
         md_move_section, md_dedupe_headings, md_lint\n\
         - Text ops: replace_text, batch_replace, search_files, apply_patch\n\
         - File ops: create_file, read_file, delete_file, move_file, append_file, \
         prepend_file, fix_whitespace, batch_tidy, git_status\n",
    );
    // Continuation lines after `\` discard leading whitespace. Start each
    // push_str body on the category marker so we do not inject indent spaces.
    #[cfg(feature = "ast")]
    s.push_str(
        "- AST ops (code-aware, 20 languages): ast_list, ast_read, ast_rename, \
         ast_replace, ast_search, ast_refs, ast_impact, ast_deps, ast_diff, ast_imports, \
         ast_insert, ast_wrap, ast_move, ast_reorder, ast_group, ast_extract_to_file, \
         ast_split, ast_map, ast_validate\n",
    );
    s.push_str(
        "- Plan ops: execute_plan\n\
         - Server: server_info\n\n\
         Use doc_* tools for parser-backed JSON/YAML/TOML mutations by selector path \
         (e.g. doc_set for setting values, doc_merge for merging objects). Use replace_text \
         only for literal or regex text replacement where structure does not matter.",
    );
    s
}

impl ServerHandler for PatchloomService {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(server_instructions());
        info.server_info.name = "patchloom".into();
        info.server_info.version = env!("CARGO_PKG_VERSION").into();
        info
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: self.tool_router.list_all(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
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
        self.log_tool_call(&tool_name, duration_ms, &result);
        result
    }
}

/// Run the MCP server over Streamable HTTP (optionally with TLS).
#[cfg(feature = "mcp-http")]
pub(crate) fn run_mcp_http_server(
    global: &GlobalFlags,
    log: Option<String>,
    host: &str,
    port: u16,
    tls_cert: Option<&std::path::Path>,
    tls_key: Option<&std::path::Path>,
) -> anyhow::Result<u8> {
    use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
    use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
    use tokio_util::sync::CancellationToken;

    let cwd = global.resolve_cwd()?;
    let ct = CancellationToken::new();

    let mut config =
        StreamableHttpServerConfig::default().with_cancellation_token(ct.child_token());

    // When binding to non-loopback, allow any Host header
    if host != "127.0.0.1" && host != "::1" && host != "localhost" {
        config = config.disable_allowed_hosts();
    }

    let log_path = log;
    let service = StreamableHttpService::new(
        move || PatchloomService::new(cwd.clone(), log_path.clone()).map_err(std::io::Error::other),
        std::sync::Arc::new(LocalSessionManager::default()),
        config,
    );

    let app = axum::Router::new().nest_service("/mcp", service);
    let addr: std::net::SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid bind address: {e}"))?;

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        if let (Some(cert), Some(key)) = (tls_cert, tls_key) {
            let tls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key)
                .await
                .map_err(|e| anyhow::anyhow!("TLS config error: {e}"))?;

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
                if let Some(real_addr) = h_addr.listening().await {
                    eprintln!("MCP HTTPS server listening on https://{real_addr}/mcp");
                }
            });

            axum_server::bind_rustls(addr, tls_config)
                .handle(handle)
                .serve(app.into_make_service())
                .await
                .map_err(|e| anyhow::anyhow!("HTTPS server error: {e}"))?;
        } else {
            let ct2 = ct.clone();
            tokio::spawn(async move {
                tokio::signal::ctrl_c().await.ok();
                ct2.cancel();
            });

            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .map_err(|e| anyhow::anyhow!("failed to bind {addr}: {e}"))?;
            eprintln!(
                "MCP HTTP server listening on http://{}/mcp",
                listener.local_addr()?
            );

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
