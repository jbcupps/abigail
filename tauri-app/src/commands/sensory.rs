use abigail_capabilities::sensory::file_ingestion::ingest_file;
use std::path::Path;

#[tauri::command]
pub fn upload_chat_attachment(file_path: String) -> Result<serde_json::Value, String> {
    let path = Path::new(&file_path);
    let result = ingest_file(path).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "content": result.content,
        "content_type": result.content_type,
        "filename": result.filename,
        "size_bytes": result.size_bytes,
        "truncated": result.truncated,
    }))
}
