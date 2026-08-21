use arc_admin_backend::openapi;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/openapi.json"));
    let document = serde_json::to_string_pretty(&openapi::document())?;
    std::fs::write(&output, format!("{document}\n"))?;
    println!("OpenAPI 已生成: {}", output.display());
    Ok(())
}
