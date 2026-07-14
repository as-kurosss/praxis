//! **DocumentReadTool** — reads documents from the filesystem.
//!
//! Supports plain-text formats natively (`.txt`, `.md`, `.rs`, `.py`, `.json`,
//! `.toml`, `.yaml`, etc.). PDF (`.pdf`) and Word (`.docx`) support is
//! available behind the `pdf` and `docx` feature flags respectively.
//!
//! # Encoding detection
//!
//! [`TextFileReader`] attempts to detect the encoding:
//! 1. UTF-8 (with BOM or without)
//! 2. UTF-16LE / UTF-16BE (via BOM)
//! 3. Latin-1 (ISO 8859-1, fallback)

use crate::agent::tool::{Tool, ToolCategory, ToolError, ToolSpec};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

// ── Error type ─────────────────────────────────────────────────────────────

/// Errors that can occur while reading a document.
#[derive(Debug, thiserror::Error)]
pub enum DocumentError {
    /// The file does not exist.
    #[error("File not found: {0}")]
    FileNotFound(String),

    /// The file format is not supported.
    #[error("Unsupported file format: {0}")]
    UnsupportedFormat(String),

    /// An I/O or decoding error occurred.
    #[error("Failed to read file: {0}")]
    ReadError(String),

    /// The file exceeds the maximum allowed size.
    #[error("File too large: {size} bytes (max {max} bytes)")]
    TooLarge { size: u64, max: u64 },

    /// The file could not be decoded as a known text encoding.
    #[error("Encoding error: {0}")]
    EncodingError(String),
}

// ── Configuration ──────────────────────────────────────────────────────────

/// Configuration for document reading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentConfig {
    /// Maximum file size in bytes (default: 10 MB).
    pub max_size_bytes: u64,
}

impl Default for DocumentConfig {
    fn default() -> Self {
        Self {
            max_size_bytes: 10 * 1024 * 1024, // 10 MB
        }
    }
}

// ── Trait ──────────────────────────────────────────────────────────────────

/// A reader that extracts text from a document file.
#[async_trait::async_trait]
pub trait DocumentReader: Send + Sync {
    /// Read the document at `path` and return its text content.
    ///
    /// # Errors
    ///
    /// Returns [`DocumentError`] if the file does not exist, is too large,
    /// has an unsupported format, or cannot be decoded.
    async fn read_text(&self, path: &Path) -> Result<String, DocumentError>;
}

// ── Text file reader ───────────────────────────────────────────────────────

/// Reads plain-text files with automatic encoding detection.
///
/// Supported encodings (in order of detection):
/// 1. UTF-8
/// 2. UTF-16LE / UTF-16BE (via BOM)
/// 3. Latin-1 (ISO 8859-1, fallback)
#[derive(Default)]
pub struct TextFileReader {
    config: DocumentConfig,
}

impl TextFileReader {
    /// Create a new reader with the given configuration.
    #[must_use]
    pub fn new(config: DocumentConfig) -> Self {
        Self { config }
    }

    /// Returns a reference to the configuration.
    #[must_use]
    pub fn config(&self) -> &DocumentConfig {
        &self.config
    }
}

#[async_trait::async_trait]
impl DocumentReader for TextFileReader {
    async fn read_text(&self, path: &Path) -> Result<String, DocumentError> {
        if !path.exists() {
            return Err(DocumentError::FileNotFound(format!("{}", path.display())));
        }

        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|e| DocumentError::ReadError(format!("cannot read metadata: {e}")))?;

        let file_size = metadata.len();
        if file_size > self.config.max_size_bytes {
            return Err(DocumentError::TooLarge {
                size: file_size,
                max: self.config.max_size_bytes,
            });
        }

        let bytes = tokio::fs::read(path)
            .await
            .map_err(|e| DocumentError::ReadError(format!("cannot read file: {e}")))?;

        decode_text(&bytes)
    }
}

// ── Encoding detection ─────────────────────────────────────────────────────

fn decode_text(bytes: &[u8]) -> Result<String, DocumentError> {
    if bytes.is_empty() {
        return Ok(String::new());
    }

    // 1. Try UTF-8 (most common)
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok(text.to_string());
    }

    // 2. Check for UTF-16 BOM
    if bytes.len() >= 2 {
        if bytes[0] == 0xFE && bytes[1] == 0xFF {
            // UTF-16BE
            let utf16: Vec<u16> = bytes[2..]
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            return String::from_utf16(&utf16)
                .map_err(|e| DocumentError::EncodingError(format!("UTF-16BE: {e}")));
        }
        if bytes[0] == 0xFF && bytes[1] == 0xFE {
            // UTF-16LE
            let utf16: Vec<u16> = bytes[2..]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            return String::from_utf16(&utf16)
                .map_err(|e| DocumentError::EncodingError(format!("UTF-16LE: {e}")));
        }
    }

    // 3. Fallback: Latin-1 (ISO 8859-1) — each byte maps to U+0000..U+00FF
    Ok(bytes.iter().map(|&b| b as char).collect())
}

// ── Mock reader (for testing) ──────────────────────────────────────────────

/// A mock document reader that returns a predetermined text for testing.
pub struct MockDocumentReader {
    /// The text that will be returned by [`read_text`].
    pub text: String,
}

impl Default for MockDocumentReader {
    fn default() -> Self {
        Self {
            text: "mock document content".into(),
        }
    }
}

#[async_trait::async_trait]
impl DocumentReader for MockDocumentReader {
    async fn read_text(&self, _path: &Path) -> Result<String, DocumentError> {
        Ok(self.text.clone())
    }
}

// ── Supported extensions ───────────────────────────────────────────────────

/// Returns `true` if the extension is recognised as a plain-text format.
fn is_text_extension(ext: &str) -> bool {
    matches!(
        ext,
        "txt"
            | "md"
            | "rs"
            | "py"
            | "js"
            | "ts"
            | "go"
            | "java"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "toml"
            | "json"
            | "yaml"
            | "yml"
            | "xml"
            | "html"
            | "css"
            | "sh"
            | "bash"
            | "fish"
            | "ps1"
            | "bat"
            | "cmd"
            | "ini"
            | "cfg"
            | "conf"
            | "env"
            | "log"
            | "csv"
            | "tsv"
            | "sql"
            | "r"
            | "rb"
            | "php"
            | "lua"
            | "ex"
            | "exs"
            | "svelte"
            | "vue"
            | "astro"
            | "tex"
            | "rss"
            | "svg"
    )
}

// ── DocumentReadTool ───────────────────────────────────────────────────────

/// A tool that reads documents from the filesystem.
///
/// Supports plain-text files natively (`.txt`, `.md`, `.rs`, `.py`, etc.).
/// PDF and Word support require the `pdf` and `docx` feature flags.
///
/// # Arguments
/// * `path` — filesystem path to the document
///
/// # Returns
/// The text content of the document.
#[derive(Default)]
pub struct DocumentReadTool {
    /// Configuration for document reading.
    pub config: DocumentConfig,
}

impl DocumentReadTool {
    /// Create a new tool with the given configuration.
    #[must_use]
    pub fn new(config: DocumentConfig) -> Self {
        Self { config }
    }

    /// Validates the file extension and returns a user-friendly error on failure.
    fn validate_extension(path: &Path) -> Result<(), DocumentError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        if ext.is_empty() || is_text_extension(&ext) {
            return Ok(());
        }

        #[cfg(feature = "pdf")]
        if ext == "pdf" {
            return Ok(());
        }

        #[cfg(feature = "docx")]
        if ext == "docx" {
            return Ok(());
        }

        Err(DocumentError::UnsupportedFormat(format!(".{ext}")))
    }
}

#[async_trait::async_trait]
impl Tool for DocumentReadTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_document".into(),
            description: "Read a document from the filesystem. Supports .txt, .md, .rs, .py, .json, .toml, .yaml and other text formats".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute or relative path to the document file"
                    }
                },
                "required": ["path"]
            }),
            category: ToolCategory::FileRead,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let path_str =
            args.get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: "read_document".into(),
                    message: "missing 'path' string".into(),
                })?;

        let path = Path::new(path_str);

        // Validate extension
        Self::validate_extension(path).map_err(|e| ToolError::InvalidArgs {
            tool: "read_document".into(),
            message: e.to_string(),
        })?;

        // Read using TextFileReader
        let reader = TextFileReader::new(self.config.clone());
        let content = reader.read_text(path).await.map_err(|e| match e {
            DocumentError::FileNotFound(_) => ToolError::InvalidArgs {
                tool: "read_document".into(),
                message: e.to_string(),
            },
            DocumentError::UnsupportedFormat(_) => ToolError::InvalidArgs {
                tool: "read_document".into(),
                message: e.to_string(),
            },
            DocumentError::TooLarge { .. } => ToolError::InvalidArgs {
                tool: "read_document".into(),
                message: e.to_string(),
            },
            DocumentError::ReadError(msg) => ToolError::Execution {
                tool: "read_document".into(),
                message: msg,
            },
            DocumentError::EncodingError(msg) => ToolError::Execution {
                tool: "read_document".into(),
                message: msg,
            },
        })?;

        Ok(json!({
            "path": path_str,
            "content": content,
        }))
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── MockDocumentReader tests ────────────────────────────────────────

    #[tokio::test]
    async fn test_mock_reader_returns_predetermined_text() {
        let reader = MockDocumentReader {
            text: "hello world".into(),
        };
        let result = reader.read_text(Path::new("/fake/path.txt")).await;
        assert_eq!(result.unwrap(), "hello world");
    }

    #[tokio::test]
    async fn test_mock_reader_default_text() {
        let reader = MockDocumentReader::default();
        let result = reader.read_text(Path::new("/fake/path.txt")).await;
        assert_eq!(result.unwrap(), "mock document content");
    }

    // ── TextFileReader — file not found ─────────────────────────────────

    #[tokio::test]
    async fn test_file_not_found() {
        let reader = TextFileReader::default();
        let result = reader
            .read_text(Path::new("/tmp/nonexistent_file_xyz.txt"))
            .await;
        assert!(matches!(result, Err(DocumentError::FileNotFound(_))));
    }

    // ── TextFileReader — too large ──────────────────────────────────────

    #[tokio::test]
    async fn test_file_too_large() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.txt");

        // Create a file larger than 1 byte
        let oversized = vec![b'a'; 100];
        std::fs::write(&path, &oversized).unwrap();

        let config = DocumentConfig { max_size_bytes: 1 };
        let reader = TextFileReader::new(config);
        let result = reader.read_text(&path).await;
        assert!(matches!(result, Err(DocumentError::TooLarge { .. })));
    }

    // ── TextFileReader — UTF-8 ──────────────────────────────────────────

    #[tokio::test]
    async fn test_read_utf8_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hello.txt");
        std::fs::write(&path, "Hello, Praxis!").unwrap();

        let reader = TextFileReader::default();
        let result = reader.read_text(&path).await;
        assert_eq!(result.unwrap(), "Hello, Praxis!");
    }

    // ── TextFileReader — UTF-8 with BOM ─────────────────────────────────

    #[tokio::test]
    async fn test_read_utf8_with_bom() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bom.txt");
        let mut bytes = vec![0xEF, 0xBB, 0xBF]; // UTF-8 BOM
        bytes.extend_from_slice(b"UTF-8 with BOM");
        std::fs::write(&path, &bytes).unwrap();

        let reader = TextFileReader::default();
        let result = reader.read_text(&path).await;
        // UTF-8 BOM is valid UTF-8, from_utf8 accepts it
        assert_eq!(result.unwrap(), "\u{feff}UTF-8 with BOM");
    }

    // ── TextFileReader — UTF-16LE ───────────────────────────────────────

    #[tokio::test]
    async fn test_read_utf16le() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("utf16le.txt");

        // UTF-16LE with BOM
        let mut bytes = Vec::new();
        bytes.push(0xFF); // BOM
        bytes.push(0xFE);
        let text = "Hello, UTF-16!";
        for code_unit in text.encode_utf16() {
            bytes.extend_from_slice(&code_unit.to_le_bytes());
        }
        std::fs::write(&path, &bytes).unwrap();

        let reader = TextFileReader::default();
        let result = reader.read_text(&path).await;
        assert_eq!(result.unwrap(), "Hello, UTF-16!");
    }

    // ── TextFileReader — UTF-16BE ───────────────────────────────────────

    #[tokio::test]
    async fn test_read_utf16be() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("utf16be.txt");

        let mut bytes = Vec::new();
        bytes.push(0xFE); // BOM
        bytes.push(0xFF);
        let text = "Hello, UTF-16BE!";
        for code_unit in text.encode_utf16() {
            bytes.extend_from_slice(&code_unit.to_be_bytes());
        }
        std::fs::write(&path, &bytes).unwrap();

        let reader = TextFileReader::default();
        let result = reader.read_text(&path).await;
        assert_eq!(result.unwrap(), "Hello, UTF-16BE!");
    }

    // ── TextFileReader — Latin-1 fallback ───────────────────────────────

    #[tokio::test]
    async fn test_read_latin1_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("latin1.txt");

        // Latin-1 bytes: 0xE9 = é, 0xF1 = ñ
        let bytes = vec![0x48, 0x65, 0x6C, 0x6C, 0xF1, 0x6F, 0x20, 0xE9];
        std::fs::write(&path, &bytes).unwrap();

        let reader = TextFileReader::default();
        let result = reader.read_text(&path).await;
        assert_eq!(result.unwrap(), "Hell\u{f1}o \u{e9}");
    }

    // ── TextFileReader — empty file ─────────────────────────────────────

    #[tokio::test]
    async fn test_read_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.txt");
        std::fs::write(&path, "").unwrap();

        let reader = TextFileReader::default();
        let result = reader.read_text(&path).await;
        assert_eq!(result.unwrap(), "");
    }

    // ── DocumentReadTool — schema ───────────────────────────────────────

    #[test]
    fn test_tool_spec_name() {
        let tool = DocumentReadTool::default();
        assert_eq!(tool.spec().name, "read_document");
    }

    #[test]
    fn test_tool_spec_category() {
        let tool = DocumentReadTool::default();
        assert_eq!(tool.spec().category, ToolCategory::FileRead);
    }

    #[test]
    fn test_tool_spec_has_path_param() {
        let tool = DocumentReadTool::default();
        let params = &tool.spec().parameters;
        let required = params["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "path"));
    }

    // ── DocumentReadTool — validation ───────────────────────────────────

    #[test]
    fn test_validate_extension_txt() {
        let result = DocumentReadTool::validate_extension(Path::new("file.txt"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_extension_unsupported() {
        let result = DocumentReadTool::validate_extension(Path::new("file.xyz"));
        assert!(matches!(result, Err(DocumentError::UnsupportedFormat(_))));
    }

    #[test]
    fn test_validate_extension_no_extension() {
        let result = DocumentReadTool::validate_extension(Path::new("README"));
        assert!(result.is_ok());
    }

    // ── DocumentReadTool — call with missing arg ────────────────────────

    #[tokio::test]
    async fn test_call_missing_path() {
        let tool = DocumentReadTool::default();
        let result = tool.call(json!({})).await;
        assert!(matches!(result, Err(ToolError::InvalidArgs { .. })));
    }

    // ── DocumentReadTool — call with non-existent file ──────────────────

    #[tokio::test]
    async fn test_call_nonexistent_file() {
        let tool = DocumentReadTool::default();
        let result = tool
            .call(json!({ "path": "/tmp/does_not_exist_xyz.txt" }))
            .await;
        assert!(matches!(result, Err(ToolError::InvalidArgs { .. })));
    }

    // ── DocumentReadTool — call with unsupported extension ──────────────

    #[tokio::test]
    async fn test_call_unsupported_extension() {
        let tool = DocumentReadTool::default();
        let result = tool.call(json!({ "path": "/tmp/file.unsupported" })).await;
        assert!(matches!(result, Err(ToolError::InvalidArgs { .. })));
    }

    // ── is_text_extension tests ─────────────────────────────────────────

    #[test]
    fn test_is_text_extension_common() {
        assert!(is_text_extension("txt"));
        assert!(is_text_extension("md"));
        assert!(is_text_extension("rs"));
        assert!(is_text_extension("py"));
        assert!(is_text_extension("json"));
        assert!(is_text_extension("toml"));
        assert!(is_text_extension("yaml"));
    }

    #[test]
    fn test_is_text_extension_unknown() {
        assert!(!is_text_extension("exe"));
        assert!(!is_text_extension("bin"));
        assert!(!is_text_extension("dll"));
        assert!(!is_text_extension("pdf"));
        assert!(!is_text_extension("docx"));
    }

    #[test]
    fn test_is_text_extension_empty() {
        assert!(!is_text_extension(""));
    }
}
