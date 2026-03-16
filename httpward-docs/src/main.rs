// src/bin/generate-schema.rs
use schemars::schema_for;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use httpward_core::config::AppConfig;

/// Escape for general markdown text (tables, descriptions)
fn escape_md_text(s: &str) -> String {
    s.replace("|", "\\|").replace('\n', "<br>")
}

/// Escape for inline code / code spans: preserve '|' so it doesn't break table
fn escape_md_code(s: &str) -> String {
    s.replace('`', "'").replace('\n', " ")
}

/// Get a meaningful name for a variant based on its properties
fn get_variant_name(variant: &Value, index: usize) -> String {
    // Try to get name from title field
    if let Some(title) = variant.get("title").and_then(|v| v.as_str()) {
        return title.to_string();
    }
    
    // Try to extract from object with single required property (like MiddlewareConfig)
    if let Some(required) = variant.get("required").and_then(|v| v.as_array()) {
        if required.len() == 1 {
            if let Some(prop_name) = required[0].as_str() {
                // Try to get more descriptive name from the property
                if let Some(prop) = variant.get("properties").and_then(|p| p.get(prop_name)) {
                    // If the property has a name field, use it
                    if let Some(name_field) = prop.get("properties").and_then(|p| p.get("name")) {
                        if let Some(name_desc) = name_field.get("description").and_then(|d| d.as_str()) {
                            return format!("{} ({})", prop_name, name_desc);
                        }
                    }
                }
                return prop_name.to_string();
            }
        } else if required.len() == 2 {
            // Special handling for Route variants based on required fields
            let required_fields: HashSet<&str> = required.iter().filter_map(|v| v.as_str()).collect();
            if required_fields.contains("backend") {
                return "Proxy".to_string();
            } else if required_fields.contains("static_dir") {
                return "Static Files".to_string();
            } else if required_fields.contains("redirect") {
                return "Redirect".to_string();
            }
        }
    }
    
    // Try to infer from type
    if let Some(typ) = variant.get("type").and_then(|v| v.as_str()) {
        match typ {
            "string" => "String".to_string(),
            "integer" => "Integer".to_string(),
            "number" => "Number".to_string(),
            "boolean" => "Boolean".to_string(),
            "array" => {
                if let Some(items) = variant.get("items") {
                    let item_type = schema_type_summary(items);
                    format!("Array of {}", item_type)
                } else {
                    "Array".to_string()
                }
            },
            "object" => {
                // Try to get a meaningful name from properties
                if let Some(props) = variant.get("properties").and_then(|v| v.as_object()) {
                    if let Some(first_key) = props.keys().next() {
                        // Capitalize first letter
                        let mut name = first_key.chars().next().unwrap().to_uppercase().to_string();
                        if first_key.len() > 1 {
                            name.push_str(&first_key[1..]);
                        }
                        return name;
                    }
                }
                "Object".to_string()
            },
            _ => format!("Type {}", typ)
        }
    } else if variant.get("$ref").is_some() {
        // If it's a reference, use the ref name
        let ref_name = variant.get("$ref").and_then(|v| v.as_str()).unwrap_or("");
        resolve_ref_name(ref_name)
    } else if variant.get("enum").is_some() {
        // If it's an enum, mention that
        "Enum".to_string()
    } else {
        // Fallback to index-based name
        format!("Option {}", index + 1)
    }
}

/// Resolve a $ref string into simple name and anchor
fn resolve_ref_name(ref_str: &str) -> String {
    ref_str.rsplit('/').next().unwrap_or(ref_str).to_string()
}

/// Collect $defs (or definitions) into a owned map for easier lookup
fn collect_defs(schema: &Value) -> HashMap<String, Value> {
    let mut map = HashMap::new();
    if let Some(defs) = schema.get("$defs").or_else(|| schema.get("definitions")) {
        if let Some(obj) = defs.as_object() {
            for (k, v) in obj {
                map.insert(k.clone(), v.clone());
            }
        }
    }
    map
}

/// Recursively find all $ref names used inside a value
fn collect_refs_in_value(v: &Value, out: &mut HashSet<String>) {
    match v {
        Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null => {}
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get("$ref") {
                out.insert(resolve_ref_name(r));
            } else {
                for (_k, val) in map {
                    collect_refs_in_value(val, out);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_refs_in_value(item, out);
            }
        }
    }
}

/// Produce a short, human-friendly summary of a schema node's type (with links for $ref)
fn schema_type_summary(node: &Value) -> String {
    if let Some(r) = node.get("$ref").and_then(|v| v.as_str()) {
        let name = resolve_ref_name(r);
        return format!("[`{}`](#{})", name, name.to_lowercase());
    }

    if let Some(t) = node.get("type") {
        match t {
            Value::String(s) => {
                if s == "array" {
                    if let Some(items) = node.get("items") {
                        return format!("array of {}", schema_type_summary(items));
                    } else {
                        return "array".to_string();
                    }
                } else {
                    return s.clone();
                }
            }
            Value::Array(arr) => {
                let types: Vec<String> = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                return types.join("|");
            }
            _ => {}
        }
    }

    if let Some(enum_v) = node.get("enum") {
        if let Some(arr) = enum_v.as_array() {
            let vals: Vec<String> = arr
                .iter()
                .map(|v| {
                    if v.is_string() {
                        format!("\"{}\"", v.as_str().unwrap())
                    } else {
                        v.to_string()
                    }
                })
                .collect();
            return format!("enum: {}", vals.join(", "));
        }
    }

    if let Some(anyof) = node.get("anyOf").or_else(|| node.get("oneOf")) {
        if let Some(arr) = anyof.as_array() {
            let parts: Vec<String> = arr.iter().map(|p| schema_type_summary(p)).collect();
            return format!("({})", parts.join(" | "));
        }
    }

    if node.get("properties").is_some() {
        return "object".to_string();
    }

    if node.get("items").is_some() {
        return format!("array of {}", schema_type_summary(&node["items"]));
    }

    if node.is_object() {
        "object".to_string()
    } else {
        "value".to_string()
    }
}

/// Format a scalar example (string/number/boolean) based on default/examples/type
fn scalar_example(schema: &Value) -> String {
    if let Some(def) = schema.get("default") {
        if def.is_string() {
            return format!("\"{}\"", def.as_str().unwrap());
        } else {
            return def.to_string();
        }
    }
    if let Some(exs) = schema.get("examples").and_then(|v| v.as_array()) {
        if let Some(first) = exs.get(0) {
            if first.is_string() {
                return format!("\"{}\"", first.as_str().unwrap());
            } else {
                return first.to_string();
            }
        }
    }
    match schema.get("type").and_then(|v| v.as_str()) {
        Some("string") => "\"...\"".to_string(),
        Some("integer") => "0".to_string(),
        Some("number") => "0.0".to_string(),
        Some("boolean") => "false".to_string(),
        Some("array") => "[]".to_string(),
        Some("object") => "{}".to_string(),
        _ => {
            if schema.get("$ref").is_some() {
                let name = resolve_ref_name(schema.get("$ref").and_then(|v| v.as_str()).unwrap());
                return format!("# see {}", name);
            }
            "\"...\"".to_string()
        }
    }
}

/// Recursive YAML example generator with reference resolution and cycle protection.
/// indent_level: number of spaces (not tabs) to use for current indentation.
fn example_for_schema_recursive(
    schema: &Value,
    defs: &HashMap<String, Value>,
    seen: &mut HashSet<String>,
    indent_level: usize,
) -> String {
    let indent = " ".repeat(indent_level);
    // If schema is a $ref — resolve it
    if let Some(ref_s) = schema.get("$ref").and_then(|v| v.as_str()) {
        let name = resolve_ref_name(ref_s);
        if seen.contains(&name) {
            return format!("# see {}", name);
        }
        if let Some(def) = defs.get(&name) {
            seen.insert(name.clone());
            let res = example_for_schema_recursive(def, defs, seen, indent_level);
            seen.remove(&name);
            return res;
        } else {
            return format!("# missing definition {}", name);
        }
    }

    // Handle anyOf/oneOf: pick first variant for example but annotate others
    if let Some(variants) = schema.get("anyOf").or_else(|| schema.get("oneOf")) {
        if let Some(arr) = variants.as_array() {
            if let Some(first) = arr.get(0) {
                let mut out = example_for_schema_recursive(first, defs, seen, indent_level);
                if arr.len() > 1 {
                    let variant_names: Vec<String> = arr.iter()
                        .enumerate()
                        .skip(1) // Skip first since it's used for example
                        .map(|(i, v)| get_variant_name(v, i))
                        .collect();
                    out.push_str(&format!("  # other options: {}\n", variant_names.join(", ")));
                }
                return out;
            }
        }
    }

    // Primitive / enum
    if let Some(enum_v) = schema.get("enum") {
        if let Some(arr) = enum_v.as_array() {
            if let Some(first) = arr.get(0) {
                if first.is_string() {
                    return format!("\"{}\"", first.as_str().unwrap());
                } else {
                    return first.to_string();
                }
            }
        }
    }

    if let Some(typ) = schema.get("type") {
        // If type is array
        if typ == "array" || (typ.is_string() && typ.as_str() == Some("array")) {
            // build example for single item
            if let Some(items) = schema.get("items") {
                let item_example = example_for_schema_recursive(items, defs, seen, 0); // Use 0 for base indent
                // If item_example is multi-line (object), format as a YAML list with proper indentation
                if item_example.contains('\n') || item_example.contains("# other options:") {
                    let mut s = String::new();
                    s.push_str("\n");
                    // Add each line with proper list indentation
                    for line in item_example.lines() {
                        if line.trim().is_empty() {
                            s.push_str(&format!("{}    \n", indent));
                        } else {
                            s.push_str(&format!("{}    {}\n", indent, line));
                        }
                    }
                    return s;
                } else {
                    return format!("[{}]", item_example);
                }
            } else {
                return "[]".to_string();
            }
        }

        // If type is object — render keys
        if typ == "object" || (typ.is_string() && typ.as_str() == Some("object")) || schema.get("properties").is_some() {
            let mut out = String::new();
            if let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
                for (field, subschema) in props {
                    let val = example_for_schema_recursive(subschema, defs, seen, 0); // Use 0 for base indent
                    // Check if value contains variant options comments
                    if val.contains("# other options:") || val.contains('\n') {
                        // multi-line example — attach on next line with indentation
                        out.push_str(&format!("{}{}:{}\n", indent, field, ""));
                        // ensure each line of val gets extra indentation
                        for line in val.lines() {
                            if line.trim().is_empty() {
                                out.push_str(&format!("{}  \n", indent));
                            } else {
                                out.push_str(&format!("{}  {}\n", indent, line));
                            }
                        }
                    } else {
                        // simple single-line value
                        out.push_str(&format!("{}{}: {}\n", indent, field, val));
                    }
                }
            } else if let Some(additional) = schema.get("additionalProperties") {
                // Allow arbitrary keys — show one example key
                let val = example_for_schema_recursive(additional, defs, seen, indent_level + 2);
                out.push_str(&format!("{}<key>: {}\n", indent, val));
            } else {
                // empty object
                out.push_str(&format!("{}{}\n", indent, "{}"));
            }
            return out;
        }

        // fallback to scalar example for string/integer/boolean
        if typ.is_string() {
            return scalar_example(schema);
        }
    }

    // If no explicit type, but has properties/items, handle accordingly
    if schema.get("properties").is_some() {
        return example_for_schema_recursive(&Value::Object(serde_json::Map::new()), defs, seen, indent_level);
    }
    if schema.get("items").is_some() {
        if let Some(items) = schema.get("items") {
            return example_for_schema_recursive(items, defs, seen, indent_level);
        }
    }

    // Default fallback
    scalar_example(schema)
}

/// Render a single definition ($defs) into Markdown with table and example YAML,
/// and also produce a 'References' row listing other defs used by this def.
fn render_definition(name: &str, def: &Value, defs_map: &HashMap<String, Value>) -> String {
    let mut out = String::new();
    
    // Add anchor for navigation
    out.push_str(&format!("<a id=\"{}\"></a>\n\n", name.to_lowercase()));
    out.push_str(&format!("## `{}`\n\n", name));

    if let Some(desc) = def.get("description").and_then(|v| v.as_str()) {
        if !desc.trim().is_empty() {
            out.push_str(&format!("{}\n\n", escape_md_text(desc)));
        }
    }

    // Collect required fields
    let required: HashSet<String> = def
        .get("required")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_else(HashSet::new);

    // Build properties table
    if let Some(props) = def.get("properties").and_then(|v| v.as_object()) {
        out.push_str("| Field | Type | Description | Default | Required |\n");
        out.push_str("|---|---|---|---|---|\n");
        for (field, schema) in props {
            let t = schema_type_summary(schema);
            let desc = schema.get("description").and_then(|v| v.as_str()).unwrap_or("");
            // Prefer explicit default shown as example (for more readable YAML)
            let default = if schema.get("default").is_some() || schema.get("examples").is_some() {
                // use scalar_example if scalar else nested marker
                example_for_schema_recursive(schema, defs_map, &mut HashSet::new(), 0)
            } else {
                scalar_example(schema)
            };
            let req = if required.contains(field) { "yes" } else { "no" };
            out.push_str(&format!(
                "| `{}` | `{}` | {} | `{}` | {} |\n",
                escape_md_text(field),
                escape_md_code(&t),
                if desc.is_empty() { "".to_string() } else { escape_md_text(desc) },
                escape_md_code(&default.replace('\n', " ").replace("  ", " ")),
                req
            ));
        }
        out.push_str("\n");

        // Example YAML (multi-line)
        out.push_str("Example (YAML):\n\n```yaml\n");
        // Build example map for this def
        let mut seen = HashSet::new();
        let example = example_for_schema_recursive(def, defs_map, &mut seen, 0);
        // example may already contain root indentation; trim trailing newline
        out.push_str(&example.trim_end());
        out.push_str("\n```\n\n");
    } else if def.get("oneOf").is_some() || def.get("anyOf").is_some() {
        // If this def is a union — render variants
        out.push_str("### Variants\n\n");
        let variants = def.get("oneOf").or_else(|| def.get("anyOf")).unwrap();
        if let Some(arr) = variants.as_array() {
            for (i, var) in arr.iter().enumerate() {
                let variant_name = get_variant_name(var, i);
                out.push_str(&format!("#### {}\n\n", variant_name));
                // If variant is object with properties, render a mini-table (reuse render_definition approach)
                if var.get("properties").is_some() {
                    out.push_str(&render_definition(&format!("{} - {}", name, variant_name), var, defs_map));
                } else {
                    out.push_str(&format!("- `{}`\n\n", escape_md_code(&schema_type_summary(var))));
                }
            }
        }
    }

    // References: list other $defs referenced from this def
    let mut refs = HashSet::new();
    collect_refs_in_value(def, &mut refs);
    // remove self if present
    refs.remove(name);
    if !refs.is_empty() {
        let mut ref_list: Vec<String> = refs.into_iter().collect();
        ref_list.sort();
        out.push_str("**References:** ");
        let links: Vec<String> = ref_list
            .iter()
            .map(|r| format!("[`{}`](#{})", r, r.to_lowercase()))
            .collect();
        out.push_str(&format!("{}\n\n", links.join(", ")));
    }

    // Add "Back to top" link
    out.push_str("[↑ Back to top](#table-of-contents)\n\n");

    out
}

/// Render full schema to Markdown, including top-level structure and sorted $defs
fn render_schema_to_markdown(schema: &Value) -> String {
    let mut md = String::new();

    // Front-matter for mkdocs-material
    md.push_str("---\n");
    md.push_str("title: \"Configuration\"\n");
    md.push_str("description: \"AppConfig — Combined configuration (global + sites).\"\n");
    md.push_str("keywords: [\"config\", \"schema\", \"httpward\"]\n");
    md.push_str("---\n\n");

    let title = schema.get("title").and_then(|v| v.as_str()).unwrap_or("Schema");
    let description = schema.get("description").and_then(|v| v.as_str()).unwrap_or("");

    md.push_str(&format!("# {} — Configuration\n\n", escape_md_text(title)));
    if !description.is_empty() {
        md.push_str(&format!("> {}\n\n", escape_md_text(description)));
    }

    // Table of Contents
    md.push_str("## Table of Contents\n\n");
    md.push_str("- [Top-level structure](#top-level-structure)\n");
    
    let defs_map = collect_defs(schema);
    if !defs_map.is_empty() {
        let mut keys: Vec<_> = defs_map.keys().cloned().collect();
        keys.sort();
        for key in keys {
            md.push_str(&format!("- [`{}`](#{})\n", key, key.to_lowercase()));
        }
    }
    md.push_str("\n---\n\n");

    // Top-level properties
    md.push_str("## Top-level structure\n\n");
    if let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
        for (name, prop) in props {
            let t = schema_type_summary(prop);
            let desc = prop.get("description").and_then(|v| v.as_str()).unwrap_or("");
            md.push_str(&format!(
                "- **`{}`** — {}{}\n",
                escape_md_text(name),
                escape_md_code(&t),
                if desc.is_empty() {
                    "".to_string()
                } else {
                    format!(" — {}", escape_md_text(desc))
                }
            ));
        }
    }

    md.push_str("\n---\n\n");

    // Definitions
    let defs_map = collect_defs(schema);
    if !defs_map.is_empty() {
        md.push_str("## Definitions\n\n");
        let mut keys: Vec<_> = defs_map.keys().cloned().collect();
        keys.sort();
        for k in keys {
            if let Some(def) = defs_map.get(&k) {
                md.push_str(&render_definition(&k, def, &defs_map));
            }
        }
    }

    md
}

fn main() -> std::io::Result<()> {
    // Generate JSON Schema for AppConfig
    let schema = schema_for!(AppConfig);
    let json_schema = serde_json::to_string_pretty(&schema).expect("Failed to serialize schema");

    let docs_dir = Path::new("C:/myprojects/HttpWard/docs");
    fs::create_dir_all(docs_dir)?;

    // Write JSON schema
    let schema_path = docs_dir.join("config.schema.json");
    fs::write(&schema_path, &json_schema)?;
    println!("Schema successfully written → {}", schema_path.display());

    // Parse schema for Markdown generation
    let schema_value: Value = serde_json::from_str(&json_schema).expect("Failed to parse JSON schema");

    // Render and write Markdown
    let md = render_schema_to_markdown(&schema_value);
    let md_path = docs_dir.join("configuration.md");
    fs::write(&md_path, md)?;
    println!("Markdown documentation successfully written → {}", md_path.display());

    Ok(())
}
