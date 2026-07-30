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
         Explore with search_files/read_file (relative paths); shell cat/find/sed \
         only for build/test unless the user overrides. MCP rejects absolute paths.\n\n\
         Core tools:\n\
         - read_file, search_files: inspect and find content\n\
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
         search_files/read_file over shell cat/find/sed when MCP is connected; \
         shell for build/test/run unless the user overrides. Prefer 'execute_plan' (or tx plans) \
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
         - Text ops: replace_text, batch_replace, search_files, apply_fragment, apply_patch\n\
         - File ops: create_file, read_file, delete_file, move_file, append_file, \
         prepend_file, fix_whitespace, batch_tidy, git_status\n",
    );
    // Continuation lines after `\` discard leading whitespace. Start each
    // push_str body on the category marker so we do not inject indent spaces.
    #[cfg(feature = "ast")]
    s.push_str(
        "- AST ops (code-aware, 20 languages): ast_list, ast_read, ast_rename, \
         ast_replace, ast_rewrite_signature, ast_search, ast_refs, ast_impact, ast_deps, ast_diff, ast_imports, \
         ast_insert, ast_wrap, ast_move, ast_reorder, ast_group, ast_extract_to_file, \
         ast_split, ast_map, ast_validate\n",
    );
    s.push_str(
        "- Plan ops: execute_plan\n\
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
    let addr: std::net::SocketAddr = format!("{host}:{port}").parse().map_err(|e| {
        anyhow::Error::new(crate::exit::InvalidInputError {
            msg: format!("invalid bind address: {e}"),
        })
    })?;

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        if let (Some(cert), Some(key)) = (tls_cert, tls_key) {
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
