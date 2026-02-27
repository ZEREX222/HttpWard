// src/bin/generate-schema.rs

use schemars::schema_for;
use serde_json;

use httpward::config::AppConfig;

fn main() -> std::io::Result<()> {
    let schema = schema_for!(AppConfig);

    let json = serde_json::to_string_pretty(&schema)
        .expect("Failed to serialize schema");

    // 1. Create the docs directory if it does not exist
    // The recursive flag ensures the full directory path is created
    std::fs::create_dir_all("docs")?;

    // 2. Write the schema file
    std::fs::write("docs/config.schema.json", json)?;

    println!("Schema successfully written to → docs/config.schema.json");

    Ok(())
}
