// src/bin/generate-schema.rs

use schemars::schema_for;
use serde_json;

use http_ward::config::AppConfig;

fn main() -> std::io::Result<()> {
    let schema = schema_for!(AppConfig);

    let json = serde_json::to_string_pretty(&schema)
        .expect("Не удалось сериализовать схему");

    // 1. Создаем папку docs, если её нет
    // Параметр recursive: true создаст всю цепочку папок
    std::fs::create_dir_all("docs")?;

    // 2. Теперь записываем файл
    std::fs::write("docs/config.schema.json", json)?;

    println!("Схема успешно сохранена → docs/config.schema.json");

    Ok(())
}
