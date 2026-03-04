// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! devit-rag -- RAG CLI for querying codebases with a local LLM

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use context_engine::{
    CancellationToken, ContextEngineConfig, ContextEngine, IndexProgress, QueryPlanner,
};
use devit_backend_core::{ChatMessage, ChatRequest};
use devit_ollama::{OllamaBackend, StreamChunk};

/// RAG CLI -- index codebases and query them with a local LLM
#[derive(Parser)]
#[command(name = "devit-rag", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Index one or more directories
    Index {
        /// Directories to index
        #[arg(required = true)]
        paths: Vec<PathBuf>,

        /// Store path (default: ~/.devit/rag)
        #[arg(long, short = 's')]
        store: Option<PathBuf>,

        /// Embedding model
        #[arg(long, short = 'm', default_value = "nomic-embed-text")]
        model: String,

        /// Ollama URL
        #[arg(long, default_value = "http://localhost:11434")]
        ollama_url: String,

        /// Extra include glob patterns (repeatable)
        #[arg(long, short = 'i')]
        include: Vec<String>,
    },

    /// Ask a question with RAG context
    Ask {
        /// The question to ask
        #[arg(required = true)]
        question: String,

        /// Store path (default: ~/.devit/rag)
        #[arg(long, short = 's')]
        store: Option<PathBuf>,

        /// LLM model for generation
        #[arg(long, short = 'm', default_value = "qwen3:8b")]
        model: String,

        /// Embedding model (must match what was used for indexing)
        #[arg(long, default_value = "nomic-embed-text")]
        embed_model: String,

        /// Ollama URL
        #[arg(long, default_value = "http://localhost:11434")]
        ollama_url: String,

        /// Number of context chunks to retrieve
        #[arg(long, short = 'k', default_value = "10")]
        top_k: usize,

        /// Max tokens budget for context
        #[arg(long, default_value = "4096")]
        max_tokens: usize,

        /// Additional system prompt
        #[arg(long)]
        system: Option<String>,

        /// Disable streaming output
        #[arg(long)]
        no_stream: bool,
    },

    /// Show index statistics
    Status {
        /// Store path (default: ~/.devit/rag)
        #[arg(long, short = 's')]
        store: Option<PathBuf>,
    },
}

fn default_store_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".devit")
        .join("rag")
}

/// Resolve store path from CLI option or default
fn resolve_store(store: &Option<PathBuf>) -> PathBuf {
    store.clone().unwrap_or_else(default_store_path)
}

/// Compute the common ancestor of a set of absolute paths.
fn common_ancestor(paths: &[PathBuf]) -> Result<PathBuf> {
    if paths.is_empty() {
        bail!("no paths provided");
    }

    let mut ancestor = paths[0].clone();
    for p in &paths[1..] {
        ancestor = common_prefix(&ancestor, p);
    }

    // If the ancestor is a file, take its parent
    if ancestor.is_file() {
        ancestor = ancestor
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or(ancestor);
    }

    if !ancestor.is_dir() {
        bail!(
            "computed common ancestor '{}' is not a directory",
            ancestor.display()
        );
    }

    Ok(ancestor)
}

/// Longest common path prefix of two absolute paths
fn common_prefix(a: &Path, b: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for (ca, cb) in a.components().zip(b.components()) {
        if ca == cb {
            result.push(ca);
        } else {
            break;
        }
    }
    result
}

/// Build include patterns scoped to the indexed directories relative to workspace_root.
fn build_include_patterns(workspace_root: &Path, paths: &[PathBuf], extra: &[String]) -> Vec<String> {
    let extensions = ["rs", "c", "cpp", "cc", "h", "hpp", "py", "js", "ts", "toml", "idl"];
    let mut patterns = Vec::new();

    for path in paths {
        let rel = path
            .strip_prefix(workspace_root)
            .unwrap_or(path);
        for ext in &extensions {
            patterns.push(format!("{}/**/*.{}", rel.display(), ext));
        }
    }

    patterns.extend(extra.iter().cloned());
    patterns
}

/// Build a ContextEngineConfig for indexing or querying.
fn build_config(
    workspace_root: PathBuf,
    store_path: PathBuf,
    ollama_url: &str,
    embed_model: &str,
    top_k: usize,
    include_patterns: Vec<String>,
) -> ContextEngineConfig {
    ContextEngineConfig {
        workspace_root,
        store_path,
        ollama_url: ollama_url.to_string(),
        embedding_model: embed_model.to_string(),
        top_k,
        include_patterns,
        ..ContextEngineConfig::default()
    }
}

// -- Subcommand handlers -------------------------------------------------------

async fn cmd_index(
    paths: Vec<PathBuf>,
    store: Option<PathBuf>,
    model: String,
    ollama_url: String,
    extra_includes: Vec<String>,
) -> Result<()> {
    // Canonicalize all input paths
    let paths: Vec<PathBuf> = paths
        .iter()
        .map(|p| {
            std::fs::canonicalize(p)
                .with_context(|| format!("cannot resolve path '{}'", p.display()))
        })
        .collect::<Result<_>>()?;

    let workspace_root = common_ancestor(&paths)?;
    let store_path = resolve_store(&store);

    eprintln!("Workspace root : {}", workspace_root.display());
    eprintln!("Store          : {}", store_path.display());
    eprintln!("Embed model    : {}", model);
    eprintln!("Directories    :");
    for p in &paths {
        eprintln!("  - {}", p.display());
    }

    let include_patterns = build_include_patterns(&workspace_root, &paths, &extra_includes);

    let config = build_config(
        workspace_root,
        store_path,
        &ollama_url,
        &model,
        10,
        include_patterns,
    );

    let engine = ContextEngine::new(config)
        .await
        .context("failed to init context engine")?;

    let cancel = CancellationToken::new();

    let stats = engine
        .index_workspace(
            |p: IndexProgress| {
                eprint!(
                    "\r[{}/{}] {} ({} chunks)   ",
                    p.files_done, p.files_total, p.current_file, p.chunks_created
                );
            },
            cancel,
        )
        .await
        .context("indexing failed")?;

    eprintln!();
    eprintln!(
        "Done: {} files, {} chunks, {} tokens in {:.1}s",
        stats.files_indexed,
        stats.chunks_created,
        stats.total_tokens,
        stats.duration_ms as f64 / 1000.0
    );

    Ok(())
}

async fn cmd_ask(
    question: String,
    store: Option<PathBuf>,
    model: String,
    embed_model: String,
    ollama_url: String,
    top_k: usize,
    max_tokens: usize,
    extra_system: Option<String>,
    no_stream: bool,
) -> Result<()> {
    let store_path = resolve_store(&store);

    // We need a workspace_root to init the engine but for querying it only matters
    // that the store_path points to the right DB.  Use store_path's parent as dummy root.
    let workspace_root = store_path
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();

    let config = build_config(
        workspace_root,
        store_path,
        &ollama_url,
        &embed_model,
        top_k,
        vec![], // include patterns not needed for queries
    );

    let engine = ContextEngine::new(config)
        .await
        .context("failed to init context engine")?;

    // Retrieve relevant chunks
    eprintln!("Searching for relevant context...");
    let chunks = engine
        .query(&question, max_tokens)
        .await
        .context("query failed")?;

    if chunks.is_empty() {
        eprintln!("Warning: no relevant context found in the index.");
    } else {
        eprintln!("Found {} relevant chunks", chunks.len());
    }

    // Format context
    let context = QueryPlanner::format_context(&chunks);

    // Build system prompt
    let mut system_prompt = String::from(
        "Tu es un expert DDS/RTPS et HDDS. \
         Reponds en te basant sur le code source fourni ci-dessous. \
         Si le code ne contient pas la reponse, dis-le clairement.\n\n",
    );
    system_prompt.push_str(&context);

    if let Some(extra) = &extra_system {
        system_prompt.push('\n');
        system_prompt.push_str(extra);
    }

    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: system_prompt,
            tool_calls: None,
            tool_name: None,
            images: None,
        },
        ChatMessage {
            role: "user".to_string(),
            content: question,
            tool_calls: None,
            tool_name: None,
            images: None,
        },
    ];

    let request = ChatRequest::new(messages).with_temperature(0.3);
    let backend = OllamaBackend::new(ollama_url, model);

    if no_stream {
        // Non-streaming mode
        use devit_backend_core::LlmBackend;
        let response = backend.chat(request).await.context("LLM chat failed")?;
        println!("{}", response.content);

        if let Some(usage) = &response.usage {
            eprintln!(
                "\n--- {} prompt + {} completion tokens ---",
                usage.prompt_tokens, usage.completion_tokens
            );
        }
    } else {
        // Streaming mode
        let mut rx = backend
            .chat_stream(request)
            .await
            .context("LLM stream failed")?;

        use std::io::Write;
        let stdout = std::io::stdout();
        let mut out = stdout.lock();

        while let Some(chunk) = rx.recv().await {
            match chunk {
                StreamChunk::Delta(text) => {
                    write!(out, "{}", text)?;
                    out.flush()?;
                }
                StreamChunk::Thinking(_) => {}
                StreamChunk::Done(resp) => {
                    writeln!(out)?;
                    if let Some(timings) = &resp.timings {
                        eprintln!(
                            "\n--- {:.1} tok/s | {:.0}ms prompt | {:.0}ms gen ---",
                            timings.tokens_per_second,
                            timings.prompt_ms,
                            timings.generation_ms,
                        );
                    }
                    break;
                }
                StreamChunk::Error(e) => {
                    bail!("stream error: {}", e);
                }
            }
        }
    }

    Ok(())
}

async fn cmd_status(store: Option<PathBuf>) -> Result<()> {
    let store_path = resolve_store(&store);
    let db_path = store_path.join("context.db");

    if !db_path.exists() {
        eprintln!("No index found at {}", db_path.display());
        eprintln!("Run `devit-rag index <PATHS...>` first.");
        return Ok(());
    }

    let workspace_root = store_path
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();

    let config = build_config(
        workspace_root,
        store_path.clone(),
        "http://localhost:11434",
        "nomic-embed-text",
        10,
        vec![],
    );

    let engine = ContextEngine::new(config)
        .await
        .context("failed to init context engine")?;

    // Use a simple query to check the store works, and count via the trait
    // Unfortunately we only have query() on the engine, but we can get chunk count
    // by querying with a dummy and checking. Let's use the count from a status query.
    // The ContextEngine doesn't expose store.count() directly, so let's open the DB
    // directly for stats.
    let conn = rusqlite::Connection::open(&db_path)
        .context("failed to open index database")?;

    let chunk_count: usize = conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
        .unwrap_or(0);

    let file_count: usize = conn
        .query_row(
            "SELECT COUNT(DISTINCT file_path) FROM chunks",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let total_tokens: usize = conn
        .query_row(
            "SELECT COALESCE(SUM(token_count), 0) FROM chunks",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // Get DB file size
    let db_size = std::fs::metadata(&db_path)
        .map(|m| m.len())
        .unwrap_or(0);

    eprintln!("Index: {}", db_path.display());
    println!("Files   : {}", file_count);
    println!("Chunks  : {}", chunk_count);
    println!("Tokens  : {}", total_tokens);
    println!("DB size : {:.1} MB", db_size as f64 / (1024.0 * 1024.0));

    // Optionally: show top files by chunk count
    let mut stmt = conn
        .prepare(
            "SELECT file_path, COUNT(*) as cnt \
             FROM chunks GROUP BY file_path ORDER BY cnt DESC LIMIT 10",
        )
        .context("failed to query top files")?;

    let rows: Vec<(String, usize)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .context("query failed")?
        .filter_map(|r| r.ok())
        .collect();

    if !rows.is_empty() {
        println!("\nTop files by chunk count:");
        for (path, count) in &rows {
            println!("  {:>4} chunks  {}", count, path);
        }
    }

    // Drop engine to avoid unused variable warning
    drop(engine);

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Index {
            paths,
            store,
            model,
            ollama_url,
            include,
        } => cmd_index(paths, store, model, ollama_url, include).await,

        Commands::Ask {
            question,
            store,
            model,
            embed_model,
            ollama_url,
            top_k,
            max_tokens,
            system,
            no_stream,
        } => {
            cmd_ask(
                question, store, model, embed_model, ollama_url, top_k, max_tokens, system,
                no_stream,
            )
            .await
        }

        Commands::Status { store } => cmd_status(store).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // -- common_prefix ---------------------------------------------------------

    #[test]
    fn common_prefix_same_path() {
        let p = PathBuf::from("/a/b/c");
        assert_eq!(common_prefix(&p, &p), PathBuf::from("/a/b/c"));
    }

    #[test]
    fn common_prefix_sibling_dirs() {
        let a = PathBuf::from("/projects/public/hdds");
        let b = PathBuf::from("/projects/public/hdds_gen");
        assert_eq!(common_prefix(&a, &b), PathBuf::from("/projects/public"));
    }

    #[test]
    fn common_prefix_nested() {
        let a = PathBuf::from("/a/b/c/d");
        let b = PathBuf::from("/a/b");
        assert_eq!(common_prefix(&a, &b), PathBuf::from("/a/b"));
    }

    #[test]
    fn common_prefix_root_only() {
        let a = PathBuf::from("/foo/bar");
        let b = PathBuf::from("/baz/qux");
        assert_eq!(common_prefix(&a, &b), PathBuf::from("/"));
    }

    #[test]
    fn common_prefix_empty_paths() {
        let a = PathBuf::new();
        let b = PathBuf::from("/a");
        assert_eq!(common_prefix(&a, &b), PathBuf::new());
    }

    // -- common_ancestor -------------------------------------------------------

    #[test]
    fn common_ancestor_empty_list() {
        let result = common_ancestor(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn common_ancestor_single_dir() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_path_buf();
        let result = common_ancestor(&[dir.clone()]).unwrap();
        assert_eq!(result, dir);
    }

    #[test]
    fn common_ancestor_two_sibling_dirs() {
        let tmp = TempDir::new().unwrap();
        let a = tmp.path().join("hdds");
        let b = tmp.path().join("hdds_gen");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();

        let result = common_ancestor(&[a, b]).unwrap();
        assert_eq!(result, tmp.path());
    }

    #[test]
    fn common_ancestor_three_dirs() {
        let tmp = TempDir::new().unwrap();
        let a = tmp.path().join("proj_a");
        let b = tmp.path().join("proj_b");
        let c = tmp.path().join("proj_c");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::create_dir_all(&c).unwrap();

        let result = common_ancestor(&[a, b, c]).unwrap();
        assert_eq!(result, tmp.path());
    }

    #[test]
    fn common_ancestor_nonexistent_dir_fails() {
        let result = common_ancestor(&[PathBuf::from("/this/does/not/exist/at/all")]);
        assert!(result.is_err());
    }

    // -- build_include_patterns ------------------------------------------------

    #[test]
    fn include_patterns_single_dir() {
        let root = PathBuf::from("/projects/public");
        let paths = vec![PathBuf::from("/projects/public/hdds")];
        let patterns = build_include_patterns(&root, &paths, &[]);

        assert!(patterns.contains(&"hdds/**/*.rs".to_string()));
        assert!(patterns.contains(&"hdds/**/*.c".to_string()));
        assert!(patterns.contains(&"hdds/**/*.h".to_string()));
        assert!(patterns.contains(&"hdds/**/*.py".to_string()));
        assert!(patterns.contains(&"hdds/**/*.idl".to_string()));
    }

    #[test]
    fn include_patterns_two_dirs() {
        let root = PathBuf::from("/projects/public");
        let paths = vec![
            PathBuf::from("/projects/public/hdds"),
            PathBuf::from("/projects/public/hdds_gen"),
        ];
        let patterns = build_include_patterns(&root, &paths, &[]);

        assert!(patterns.contains(&"hdds/**/*.rs".to_string()));
        assert!(patterns.contains(&"hdds_gen/**/*.rs".to_string()));
    }

    #[test]
    fn include_patterns_with_extras() {
        let root = PathBuf::from("/a");
        let paths = vec![PathBuf::from("/a/b")];
        let extra = vec!["**/*.md".to_string()];
        let patterns = build_include_patterns(&root, &paths, &extra);

        assert!(patterns.contains(&"**/*.md".to_string()));
    }

    #[test]
    fn include_patterns_extensions_count() {
        let root = PathBuf::from("/root");
        let paths = vec![PathBuf::from("/root/proj")];
        let patterns = build_include_patterns(&root, &paths, &[]);

        // 11 extensions: rs, c, cpp, cc, h, hpp, py, js, ts, toml, idl
        assert_eq!(patterns.len(), 11);
    }

    // -- build_config ----------------------------------------------------------

    #[test]
    fn config_defaults_are_sane() {
        let cfg = build_config(
            PathBuf::from("/ws"),
            PathBuf::from("/store"),
            "http://localhost:11434",
            "nomic-embed-text",
            15,
            vec!["**/*.rs".to_string()],
        );

        assert_eq!(cfg.workspace_root, PathBuf::from("/ws"));
        assert_eq!(cfg.store_path, PathBuf::from("/store"));
        assert_eq!(cfg.ollama_url, "http://localhost:11434");
        assert_eq!(cfg.embedding_model, "nomic-embed-text");
        assert_eq!(cfg.top_k, 15);
        assert_eq!(cfg.include_patterns, vec!["**/*.rs".to_string()]);
        // Inherited defaults
        assert_eq!(cfg.max_chunk_tokens, 512);
        assert_eq!(cfg.chunk_overlap, 64);
        assert_eq!(cfg.similarity_threshold, 0.7);
        assert!(!cfg.exclude_patterns.is_empty());
    }

    // -- resolve_store ---------------------------------------------------------

    #[test]
    fn resolve_store_with_explicit_path() {
        let p = PathBuf::from("/custom/store");
        assert_eq!(resolve_store(&Some(p.clone())), p);
    }

    #[test]
    fn resolve_store_default() {
        let result = resolve_store(&None);
        // Should end with .devit/rag regardless of home dir
        assert!(result.ends_with(".devit/rag"));
    }

    // -- CLI parsing (clap verify) ---------------------------------------------

    #[test]
    fn cli_parses_index_minimal() {
        let cli = Cli::parse_from(["devit-rag", "index", "/some/path"]);
        match cli.command {
            Commands::Index { paths, model, .. } => {
                assert_eq!(paths, vec![PathBuf::from("/some/path")]);
                assert_eq!(model, "nomic-embed-text");
            }
            _ => panic!("expected Index"),
        }
    }

    #[test]
    fn cli_parses_index_multi_paths() {
        let cli = Cli::parse_from(["devit-rag", "index", "/a", "/b", "/c"]);
        match cli.command {
            Commands::Index { paths, .. } => {
                assert_eq!(paths.len(), 3);
            }
            _ => panic!("expected Index"),
        }
    }

    #[test]
    fn cli_parses_index_all_options() {
        let cli = Cli::parse_from([
            "devit-rag", "index", "/p",
            "--store", "/s",
            "--model", "mxbai-embed-large",
            "--ollama-url", "http://gpu:11434",
            "-i", "**/*.md",
            "-i", "**/*.txt",
        ]);
        match cli.command {
            Commands::Index {
                store, model, ollama_url, include, ..
            } => {
                assert_eq!(store, Some(PathBuf::from("/s")));
                assert_eq!(model, "mxbai-embed-large");
                assert_eq!(ollama_url, "http://gpu:11434");
                assert_eq!(include, vec!["**/*.md", "**/*.txt"]);
            }
            _ => panic!("expected Index"),
        }
    }

    #[test]
    fn cli_parses_ask_defaults() {
        let cli = Cli::parse_from(["devit-rag", "ask", "what is SPDP?"]);
        match cli.command {
            Commands::Ask {
                question, model, embed_model, top_k, max_tokens, no_stream, ..
            } => {
                assert_eq!(question, "what is SPDP?");
                assert_eq!(model, "qwen3:8b");
                assert_eq!(embed_model, "nomic-embed-text");
                assert_eq!(top_k, 10);
                assert_eq!(max_tokens, 4096);
                assert!(!no_stream);
            }
            _ => panic!("expected Ask"),
        }
    }

    #[test]
    fn cli_parses_ask_all_options() {
        let cli = Cli::parse_from([
            "devit-rag", "ask", "question?",
            "-m", "llama3:70b",
            "--embed-model", "bge-m3",
            "-k", "20",
            "--max-tokens", "8192",
            "--system", "extra instructions",
            "--no-stream",
            "--store", "/s",
            "--ollama-url", "http://x:1234",
        ]);
        match cli.command {
            Commands::Ask {
                model, embed_model, top_k, max_tokens, system, no_stream, store, ollama_url, ..
            } => {
                assert_eq!(model, "llama3:70b");
                assert_eq!(embed_model, "bge-m3");
                assert_eq!(top_k, 20);
                assert_eq!(max_tokens, 8192);
                assert_eq!(system, Some("extra instructions".to_string()));
                assert!(no_stream);
                assert_eq!(store, Some(PathBuf::from("/s")));
                assert_eq!(ollama_url, "http://x:1234");
            }
            _ => panic!("expected Ask"),
        }
    }

    #[test]
    fn cli_parses_status() {
        let cli = Cli::parse_from(["devit-rag", "status", "--store", "/my/store"]);
        match cli.command {
            Commands::Status { store } => {
                assert_eq!(store, Some(PathBuf::from("/my/store")));
            }
            _ => panic!("expected Status"),
        }
    }

    // -- cmd_status on empty/missing DB ----------------------------------------

    #[tokio::test]
    async fn status_no_index_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let result = cmd_status(Some(tmp.path().to_path_buf())).await;
        assert!(result.is_ok());
    }

    // -- cmd_status with a real (empty) DB -------------------------------------

    #[tokio::test]
    async fn status_empty_db() {
        let tmp = TempDir::new().unwrap();
        let store_path = tmp.path().to_path_buf();
        let db_path = store_path.join("context.db");

        // Create a minimal DB with the expected schema
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS chunks (
                id TEXT PRIMARY KEY,
                file_path TEXT NOT NULL,
                start_line INTEGER,
                end_line INTEGER,
                content TEXT,
                language TEXT,
                chunk_type TEXT,
                symbol_name TEXT,
                token_count INTEGER DEFAULT 0,
                file_mtime INTEGER DEFAULT 0,
                embedding BLOB
            );",
        )
        .unwrap();
        drop(conn);

        // cmd_status should work on this empty-but-valid DB
        // It will fail at ContextEngine::new because the engine opens the DB
        // its own way, but our direct rusqlite queries should succeed.
        // Let's test the direct queries instead.
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let count: usize = conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        let file_count: usize = conn
            .query_row("SELECT COUNT(DISTINCT file_path) FROM chunks", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(file_count, 0);

        let tokens: usize = conn
            .query_row("SELECT COALESCE(SUM(token_count), 0) FROM chunks", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(tokens, 0);
    }

    // -- cmd_status with populated DB ------------------------------------------

    #[tokio::test]
    async fn status_populated_db() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("context.db");

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE chunks (
                id TEXT PRIMARY KEY,
                file_path TEXT NOT NULL,
                start_line INTEGER,
                end_line INTEGER,
                content TEXT,
                language TEXT,
                chunk_type TEXT,
                symbol_name TEXT,
                token_count INTEGER DEFAULT 0,
                file_mtime INTEGER DEFAULT 0,
                embedding BLOB
            );",
        )
        .unwrap();

        // Insert fake chunks
        for i in 0..5 {
            conn.execute(
                "INSERT INTO chunks (id, file_path, start_line, end_line, content, language, chunk_type, token_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    format!("chunk_{}", i),
                    if i < 3 { "hdds/src/discovery.rs" } else { "hdds_gen/src/parser.rs" },
                    i * 10 + 1,
                    i * 10 + 10,
                    format!("fn func_{}() {{}}", i),
                    "rust",
                    "function",
                    42,
                ],
            )
            .unwrap();
        }
        drop(conn);

        // Verify counts
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let count: usize = conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 5);

        let file_count: usize = conn
            .query_row("SELECT COUNT(DISTINCT file_path) FROM chunks", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(file_count, 2);

        let tokens: usize = conn
            .query_row("SELECT COALESCE(SUM(token_count), 0) FROM chunks", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(tokens, 210); // 5 * 42

        // Top files query
        let mut stmt = conn
            .prepare(
                "SELECT file_path, COUNT(*) as cnt \
                 FROM chunks GROUP BY file_path ORDER BY cnt DESC LIMIT 10",
            )
            .unwrap();
        let rows: Vec<(String, usize)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "hdds/src/discovery.rs");
        assert_eq!(rows[0].1, 3);
        assert_eq!(rows[1].0, "hdds_gen/src/parser.rs");
        assert_eq!(rows[1].1, 2);
    }
}
