use httpward_core::config::AppConfig;
use schemars::schema_for;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn escape_md_text(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', "<br>")
}

fn escape_md_code(s: &str) -> String {
    s.replace('`', "'").replace('\n', " ")
}

fn anchor_for(name: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;

    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }

    out.trim_matches('-').to_string()
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn resolve_ref_name(ref_str: &str) -> String {
    ref_str.rsplit('/').next().unwrap_or(ref_str).to_string()
}

fn collect_defs(schema: &Value) -> HashMap<String, Value> {
    let mut map = HashMap::new();

    if let Some(defs) = schema.get("$defs").or_else(|| schema.get("definitions"))
        && let Some(obj) = defs.as_object() {
            for (name, value) in obj {
                map.insert(name.clone(), value.clone());
            }
        }

    map
}

fn required_fields(node: &Value) -> BTreeSet<String> {
    node.get("required")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn union_variants(node: &Value) -> Option<&Vec<Value>> {
    node.get("oneOf")
        .or_else(|| node.get("anyOf"))
        .and_then(Value::as_array)
}

fn is_null_schema(node: &Value) -> bool {
    match node.get("type") {
        Some(Value::String(kind)) => kind == "null",
        Some(Value::Array(items)) => items.iter().any(|item| item.as_str() == Some("null")),
        _ => false,
    }
}

fn is_nullable_schema(node: &Value) -> bool {
    if let Some(types) = node.get("type").and_then(Value::as_array) {
        return types.iter().any(|item| item.as_str() == Some("null"));
    }

    union_variants(node)
        .map(|variants| variants.iter().any(is_null_schema))
        .unwrap_or(false)
}

fn literal_markdown(value: &Value) -> String {
    let raw = if let Some(s) = value.as_str() {
        format!("\"{}\"", s)
    } else {
        value.to_string()
    };

    format!("`{}`", escape_md_code(&raw))
}

fn schema_type_summary(node: &Value) -> String {
    if let Some(reference) = node.get("$ref").and_then(Value::as_str) {
        let name = resolve_ref_name(reference);
        return format!("[`{}`](#{})", name, anchor_for(&name));
    }

    if let Some(variants) = union_variants(node) {
        let non_null: Vec<&Value> = variants
            .iter()
            .filter(|variant| !is_null_schema(variant))
            .collect();

        if non_null.len() == 1 && variants.len() > 1 {
            return format!("optional {}", schema_type_summary(non_null[0]));
        }

        if !non_null.is_empty() {
            let parts: Vec<String> = non_null
                .iter()
                .map(|variant| schema_type_summary(variant))
                .collect();
            return format!("one of: {}", parts.join(" or "));
        }
    }

    if let Some(types) = node.get("type").and_then(Value::as_array) {
        let mut names: Vec<String> = types
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect();

        if names.len() == 2 && names.iter().any(|item| item == "null") {
            names.retain(|item| item != "null");
            if let Some(kind) = names.first() {
                return format!("optional `{}`", kind);
            }
        }

        return format!(
            "one of: {}",
            names
                .iter()
                .map(|kind| format!("`{}`", kind))
                .collect::<Vec<_>>()
                .join(" or ")
        );
    }

    if let Some(values) = node.get("enum").and_then(Value::as_array) {
        return format!(
            "enum: {}",
            values
                .iter()
                .map(literal_markdown)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    match node.get("type").and_then(Value::as_str) {
        Some("array") => match node.get("items") {
            Some(items) => format!("list of {}", schema_type_summary(items)),
            None => "`array`".to_string(),
        },
        Some("object") => match node.get("additionalProperties") {
            Some(Value::Bool(true)) => "map of string to values".to_string(),
            Some(Value::Bool(false)) => "`object`".to_string(),
            Some(other) => format!("map of string to {}", schema_type_summary(other)),
            None => "`object`".to_string(),
        },
        Some(kind) => format!("`{}`", kind),
        None if node.get("properties").is_some() => "`object`".to_string(),
        None if node.get("items").is_some() => match node.get("items") {
            Some(items) => format!("list of {}", schema_type_summary(items)),
            None => "`array`".to_string(),
        },
        _ => "value".to_string(),
    }
}

fn default_summary(node: &Value) -> String {
    if let Some(default) = node.get("default") {
        if default.is_object() || default.is_array() {
            return "see examples".to_string();
        }
        return literal_markdown(default);
    }

    if let Some(example) = node
        .get("examples")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
    {
        return literal_markdown(example);
    }

    if is_nullable_schema(node) {
        return "omitted".to_string();
    }

    match node.get("type").and_then(Value::as_str) {
        Some("array") => "`[]`".to_string(),
        Some("object") => "`{}`".to_string(),
        _ if node.get("additionalProperties").is_some() => "`{}`".to_string(),
        _ if node.get("$ref").is_some() || union_variants(node).is_some() => {
            "see examples".to_string()
        }
        _ => "—".to_string(),
    }
}

fn render_properties_table(node: &Value) -> String {
    let Some(properties) = node.get("properties").and_then(Value::as_object) else {
        return String::new();
    };

    let required = required_fields(node);
    let mut out = String::new();
    out.push_str("| Field | Type | Required | Default | Description |\n");
    out.push_str("|---|---|---|---|---|\n");

    for (field, schema) in properties {
        let description = schema
            .get("description")
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map(escape_md_text)
            .unwrap_or_else(|| "—".to_string());

        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            escape_md_text(field),
            schema_type_summary(schema),
            if required.contains(field) {
                "yes"
            } else {
                "no"
            },
            default_summary(schema),
            description,
        ));
    }

    out.push('\n');
    out
}

fn render_yaml_example(title: &str, yaml: &str) -> String {
    let content = yaml.trim().replace("\r\n", "\n");
    format!("### {}\n\n```yaml\n{}\n```\n\n", title, content)
}

fn render_section(title: &str, body: &str) -> String {
    format!(
        "<a id=\"{}\"></a>\n\n## {}\n\n{}\n\n",
        anchor_for(title),
        title,
        body.trim()
    )
}

fn render_object_reference_section(
    name: &str,
    intro: &str,
    def: &Value,
    examples: Vec<(String, String)>,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "<a id=\"{}\"></a>\n\n## `{}`\n\n",
        anchor_for(name),
        name
    ));

    if !intro.trim().is_empty() {
        out.push_str(intro.trim());
        out.push_str("\n\n");
    }

    out.push_str(&render_properties_table(def));

    for (title, yaml) in examples {
        out.push_str(&render_yaml_example(&title, &yaml));
    }

    out.push_str("[↑ Back to top](#table-of-contents)\n\n");
    out
}

fn route_variant_name(variant: &Value) -> &'static str {
    let required = required_fields(variant);

    if required.contains("backend") {
        "Proxy route"
    } else if required.contains("static_dir") {
        "Static files route"
    } else if required.contains("redirect") {
        "Redirect route"
    } else {
        "Route variant"
    }
}

fn route_variant_example(name: &str) -> &'static str {
    match name {
        "Proxy route" => {
            r#"match:
  path: "/api/{*path}"
backend: "http://127.0.0.1:8080/{*path}"
strategy: "default""#
        }
        "Static files route" => {
            r#"match:
  path: "/assets/{*path}"
static_dir: "C:/sites/example/assets/{*path}""#
        }
        "Redirect route" => {
            r#"match:
  path: "/old"
redirect:
  to: "https://example.com/new"
  code: 301"#
        }
        _ => "# no example",
    }
}

fn render_route_section(def: &Value) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "<a id=\"{}\"></a>\n\n## `Route`\n\n",
        anchor_for("Route")
    ));
    out.push_str("A route decides what HttpWard should do with a matching request. In practice there are three forms: proxy to an upstream, serve static files, or return a redirect.\n\n");
    out.push_str("Common fields:\n\n");
    out.push_str("- `match` — path matcher. Prefer `path` when possible; use `path_regex` only when you really need regex behavior.\n");
    out.push_str("- `strategy` — either a named strategy like `\"default\"` or an inline list of middleware.\n");
    out.push_str("- `strategies` — per-route named strategy map if you want route-local reusable strategies.\n\n");

    if let Some(variants) = union_variants(def) {
        for variant in variants {
            let title = route_variant_name(variant);
            out.push_str(&format!("### {}\n\n", title));
            out.push_str(&render_properties_table(variant));
            out.push_str(&render_yaml_example(
                "Example",
                route_variant_example(title),
            ));
        }
    }

    out.push_str("[↑ Back to top](#table-of-contents)\n\n");
    out
}

fn render_strategy_ref_section() -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "<a id=\"{}\"></a>\n\n## `StrategyRef`\n\n",
        anchor_for("StrategyRef")
    ));
    out.push_str("A strategy reference can be written in two user-friendly ways.\n\n");
    out.push_str("1. **Named strategy** — points to a strategy defined in `strategies.yml` or in a local `strategies:` map.\n");
    out.push_str("2. **Inline middleware list** — define middleware directly where the strategy is used.\n\n");
    out.push_str(&render_yaml_example(
        "Named strategy",
        "strategy: \"default\"",
    ));
    out.push_str(&render_yaml_example(
        "Inline strategy",
        r#"strategy:
  - rate_limit:
      requests: 100
      window: "1m"
  - logging:
      level: info"#,
    ));
    out.push_str("[↑ Back to top](#table-of-contents)\n\n");
    out
}

fn render_middleware_config_section() -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "<a id=\"{}\"></a>\n\n## `MiddlewareConfig`\n\n",
        anchor_for("MiddlewareConfig")
    ));
    out.push_str("Each middleware item in a strategy is a single-key YAML object. The key is the middleware name; the value is either its configuration, `off`, or `false`.\n\n");
    out.push_str(&render_yaml_example(
        "Enabled middleware",
        r#"- logging:
    level: info"#,
    ));
    out.push_str(&render_yaml_example(
        "Disable middleware with `off`",
        r#"- logging: off"#,
    ));
    out.push_str(&render_yaml_example(
        "Disable middleware with `false`",
        r#"- logging: false"#,
    ));
    out.push_str("This is especially useful when a site or route inherits a strategy from above and you want to turn one middleware off locally.\n\n");
    out.push_str("[↑ Back to top](#table-of-contents)\n\n");
    out
}

fn read_optional_file(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|content| content.trim().replace("\r\n", "\n"))
}

fn find_site_example(workspace_root: &Path) -> Option<(String, String)> {
    let sites_dir = workspace_root.join("sites-enabled");
    let entries = fs::read_dir(&sites_dir).ok()?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect();
    paths.sort();

    for path in paths {
        let extension = path.extension().and_then(|ext| ext.to_str());
        if matches!(extension, Some("yml") | Some("yaml")) {
            let name = path.file_name()?.to_string_lossy().to_string();
            let content = read_optional_file(&path)?;
            return Some((name, content));
        }
    }

    None
}

fn configuration_intro() -> &'static str {
    "This page explains the files that people actually edit: `httpward.yaml`, `strategies.yml`, and `sites-enabled/*.yml`. HttpWard loads those files into an internal [`AppConfig`](#appconfig), but the focus here is practical YAML, not raw schema internals.\n\nFor a page with copy-pasteable recipes, see [Configuration examples](configuration-examples.md)."
}

fn minimal_httpward_yaml() -> &'static str {
    r#"domain: example.com
listeners:
  - port: 80
sites_enabled: ./sites-enabled"#
}

fn multisite_httpward_yaml() -> &'static str {
    r#"log:
  level: "info"

strategy: my_custom_strategy

listeners:
  - port: 443
    tls:
      self_signed: true

routes:
  - match:
      path: "/my/{*any}"
    backend: "http://zerex222.ru:8080/{*any}"

  - match:
      path: "/site/{*path}"
    static_dir: "C:/myprojects/html/{*path}"

  - match:
      path: "/search/{request}"
    redirect:
      to: "https://www.google.com/search?q={request}"

sites_enabled: "./sites-enabled"

strategies:
  my_custom_strategy:
    - httpward_log_module:
        show_request: true
        log_client_ip: true
        log_current_site: true
        log_route_info: true
        log_response_status: true
        log_fingerprints: true"#
}

fn minimal_site_yaml() -> &'static str {
    r#"domain: app.example.com
routes:
  - match:
      path: "/"
    backend: "http://127.0.0.1:3000""#
}

fn multisite_site_yaml() -> &'static str {
    r#"domains: ["test.local", "*.test2.local"]

listeners:
  - port: 443
    tls:
      self_signed: true

strategy: default55

routes:
  - match:
      path: "/api"
    backend: "http://127.0.0.1:8080"

  - match:
      path: "/site1/{*path}"
    static_dir: "C:/myprojects/html/{*path}"

  - match:
      path: "/aaa/{id}"
    backend: "http://127.0.0.1:3000/api/{id}""#
}

fn minimal_strategies_yaml() -> &'static str {
    r#"default:
  - logging:
      level: info
  - rate_limit:
      requests: 1000
      window: "1m""#
}

fn domains_matching_notes() -> &'static str {
    "Domain resolution happens at request time and is based on the same matching logic used by the runtime:\n\n- HttpWard first tries the `Host` header (port is stripped, for example `example.com:443` becomes `example.com`).\n- If `Host` is missing, it falls back to TLS SNI (when available).\n- Matching uses wildcard patterns (`wildmatch`), so values like `*.example.com` are supported in both `domain` and `domains`.\n- Matching is effectively case-insensitive for incoming requests because runtime normalizes host/SNI to lowercase; use lowercase domains in config to avoid ambiguity.\n- If no domain matches, HttpWard can fall back to an unrestricted site (typically global routes with no `domain`/`domains`).\n- If there is no unrestricted site either, normal not-found handling is used."
}

fn render_configuration_markdown(schema: &Value, workspace_root: &Path) -> String {
    let defs = collect_defs(schema);
    let global_example = read_optional_file(&workspace_root.join("httpward.yaml"));
    let strategies_example = read_optional_file(&workspace_root.join("strategies.yml"))
        .or_else(|| read_optional_file(&workspace_root.join("strategies.yaml")));
    let site_example = find_site_example(workspace_root);

    let global_def = defs
        .get("GlobalConfig")
        .expect("GlobalConfig schema definition");
    let site_def = defs
        .get("SiteConfig")
        .expect("SiteConfig schema definition");
    let listener_def = defs.get("Listener").expect("Listener schema definition");
    let tls_def = defs.get("Tls").expect("Tls schema definition");
    let match_def = defs.get("Match").expect("Match schema definition");
    let redirect_def = defs.get("Redirect").expect("Redirect schema definition");
    let log_def = defs.get("LogConfig").expect("LogConfig schema definition");
    let route_def = defs.get("Route").expect("Route schema definition");

    let mut md = String::new();
    md.push_str("---\n");
    md.push_str("title: \"Configuration\"\n");
    md.push_str("description: \"Human-friendly guide to HttpWard configuration files.\"\n");
    md.push_str("keywords: [\"config\", \"yaml\", \"schema\", \"httpward\"]\n");
    md.push_str("---\n\n");
    md.push_str("# HttpWard Configuration\n\n");
    md.push_str(&format!("> {}\n\n", configuration_intro()));

    md.push_str("<a id=\"table-of-contents\"></a>\n\n");
    md.push_str("## Table of Contents\n\n");
    md.push_str("- [How configuration is loaded](#how-configuration-is-loaded)\n");
    md.push_str("- [Quick start](#quick-start)\n");
    md.push_str("- [How `domains` matching works](#how-domains-matching-works)\n");
    md.push_str("- [AppConfig](#appconfig)\n");
    md.push_str("- [Global file: `httpward.yaml`](#global-file-httpward-yaml)\n");
    md.push_str("- [Site files: `sites-enabled/*.yml`](#site-files-sites-enabled-yml)\n");
    md.push_str("- [Strategies file: `strategies.yml`](#strategies-file-strategies-yml)\n");
    md.push_str("- [Reusable types](#reusable-types)\n");
    md.push_str("  - [`Listener`](#listener)\n");
    md.push_str("  - [`Tls`](#tls)\n");
    md.push_str("  - [`LogConfig`](#logconfig)\n");
    md.push_str("  - [`Match`](#match)\n");
    md.push_str("  - [`Route`](#route)\n");
    md.push_str("  - [`Redirect`](#redirect)\n");
    md.push_str("  - [`StrategyRef`](#strategyref)\n");
    md.push_str("  - [`MiddlewareConfig`](#middlewareconfig)\n\n");

    md.push_str(&render_section(
        "How configuration is loaded",
        "1. HttpWard reads `httpward.yaml` as the global configuration.\n2. If `strategies.yml` exists next to it, those named strategies are loaded and merged in.\n3. HttpWard then reads all `*.yml` / `*.yaml` files from the directory referenced by `sites_enabled`.\n4. At runtime those pieces become a single `AppConfig { global, sites }`.\n\nImportant validation rules:\n\n- a site file must define either `domain` or `domains`;\n- a listener with `tls` enabled must use a non-zero `port`;\n- `strategy` can be a string name or an inline middleware list.",
    ));

    let quick_start_body = format!(
        "Use this structure when you want to split config by domains:\n\n1. Keep global listeners, shared routes, and shared strategies in `httpward.yaml`.\n2. Set `sites_enabled: \"./sites-enabled\"` in `httpward.yaml`.\n3. Put one or more site files into `sites-enabled/` (for example `sites-enabled/test.local.yml`) with `domain` or `domains`.\n\n{}{}{}{}",
        render_yaml_example("Minimal `httpward.yaml`", minimal_httpward_yaml()),
        render_yaml_example(
            "Recommended multi-site `httpward.yaml`",
            multisite_httpward_yaml()
        ),
        render_yaml_example("Minimal `strategies.yml`", minimal_strategies_yaml()),
        render_yaml_example(
            "Recommended `sites-enabled/test.local.yml`",
            multisite_site_yaml()
        ),
    );
    md.push_str(&render_section("Quick start", &quick_start_body));

    md.push_str(&render_section(
        "How `domains` matching works",
        domains_matching_notes(),
    ));

    let mut app_config_body = String::new();
    app_config_body.push_str("`AppConfig` is the combined in-memory model, not a file you write by hand. It is useful for tooling, validation, and the generated `config.schema.json`.\n\n");
    app_config_body.push_str("| Field | Type | Description |\n");
    app_config_body.push_str("|---|---|---|\n");
    app_config_body
        .push_str("| `global` | [`GlobalConfig`](#globalconfig) | Parsed from `httpward.yaml` |\n");
    app_config_body.push_str("| `sites` | list of [`SiteConfig`](#siteconfig) | Parsed from the directory configured in `sites_enabled` |\n\n");
    app_config_body.push_str(
        "The JSON Schema generated from this type is written to `docs/config.schema.json`.\n",
    );
    md.push_str(&render_section("AppConfig", &app_config_body));

    let mut global_examples = vec![(
        "Recommended starting point".to_string(),
        minimal_httpward_yaml().to_string(),
    )];
    if let Some(example) = global_example {
        global_examples.push(("Example from this repository".to_string(), example));
    }
    md.push_str(&render_object_reference_section(
        "GlobalConfig",
        "`httpward.yaml` is the main file in the project root. It defines listeners, default routing, logging, the site directory, and optional default strategies.",
        global_def,
        global_examples,
    ));

    let mut site_examples = vec![
        (
            "Minimal site file".to_string(),
            minimal_site_yaml().to_string(),
        ),
        (
            "Recommended multi-site file".to_string(),
            multisite_site_yaml().to_string(),
        ),
    ];
    if let Some((name, example)) = &site_example {
        site_examples.push((
            format!("Example from this repository: `sites-enabled/{}`", name),
            example.clone(),
        ));
    }
    md.push_str(&format!("<a id=\"{}\"></a>\n\n## Global file: `httpward.yaml`\n\nThis section documents the fields of [`GlobalConfig`](#globalconfig).\n\n[↑ Back to top](#table-of-contents)\n\n", anchor_for("Global file: `httpward.yaml`")));
    md.push_str(&format!("<a id=\"{}\"></a>\n\n## Site files: `sites-enabled/*.yml`\n\nEach file in `sites-enabled/` describes one site or virtual host. For domain-based separation, set `sites_enabled: \"./sites-enabled\"` in `httpward.yaml` and keep per-domain configs here (for example `sites-enabled/test.local.yml`, `sites-enabled/api.example.com.yml`). Site settings can override global listeners, routes, and strategies when needed.\n\n", anchor_for("Site files: `sites-enabled/*.yml`")));
    md.push_str(&render_object_reference_section(
        "SiteConfig",
        "",
        site_def,
        site_examples,
    ));

    let mut strategies_body = String::new();
    strategies_body.push_str("`strategies.yml` is a map of strategy name to an array of middleware entries. It is the best place for reusable policies such as logging, rate limiting, auth, or headers.\n\n");
    strategies_body.push_str("Structure rules:\n\n");
    strategies_body.push_str("- the top-level key is the strategy name;\n");
    strategies_body.push_str("- the value is a YAML list;\n");
    strategies_body.push_str("- each list item is a [`MiddlewareConfig`](#middlewareconfig);\n");
    strategies_body
        .push_str("- you can reference a strategy by name with [`StrategyRef`](#strategyref).\n\n");
    strategies_body.push_str(&render_yaml_example(
        "Reusable strategies",
        strategies_example
            .as_deref()
            .unwrap_or(minimal_strategies_yaml()),
    ));
    strategies_body.push_str(&render_yaml_example(
        "Disable one inherited middleware",
        r#"safe-mode:
  - rate_limit:
      requests: 10
      window: "1m"
  - logging: off"#,
    ));
    strategies_body.push_str("[↑ Back to top](#table-of-contents)\n");
    md.push_str(&render_section(
        "Strategies file: `strategies.yml`",
        &strategies_body,
    ));

    md.push_str(&render_section(
        "Reusable types",
        "The following sections document the nested building blocks reused across global config, site config, routes, listeners, and strategies.",
    ));

    md.push_str(&render_object_reference_section(
        "Listener",
        "A listener binds HttpWard to an address/port and can optionally enable TLS.",
        listener_def,
        vec![
            (
                "HTTP listener".to_string(),
                "host: \"0.0.0.0\"\nport: 80".to_string(),
            ),
            (
                "HTTPS listener with self-signed certificate".to_string(),
                "host: \"0.0.0.0\"\nport: 443\ntls:\n  self_signed: true".to_string(),
            ),
        ],
    ));
    md.push_str(&render_object_reference_section(
        "Tls",
        "TLS settings for a listener. Use `self_signed: true` for local development; in production prefer explicit `cert` and `key` paths.",
        tls_def,
        vec![
            (
                "Development TLS".to_string(),
                "self_signed: true".to_string(),
            ),
            (
                "Certificate files".to_string(),
                "self_signed: false\ncert: \"C:/certs/fullchain.pem\"\nkey: \"C:/certs/privkey.pem\"".to_string(),
            ),
        ],
    ));
    md.push_str(&render_object_reference_section(
        "LogConfig",
        "Logging settings used by the built-in logging module and related middleware.",
        log_def,
        vec![("Recommended default".to_string(), "level: warn".to_string())],
    ));
    md.push_str(&render_object_reference_section(
        "Match",
        "A path matcher used by routes. Prefer `path` for readability and performance. Reach for `path_regex` only when pattern routing cannot be expressed with path templates.",
        match_def,
        vec![
            (
                "Template path".to_string(),
                "path: \"/api/{*path}\"".to_string(),
            ),
            (
                "Regex path".to_string(),
                "path_regex: \"^/v[0-9]+/api\"".to_string(),
            ),
        ],
    ));
    md.push_str(&render_route_section(route_def));
    md.push_str(&render_object_reference_section(
        "Redirect",
        "Redirect target used by redirect routes.",
        redirect_def,
        vec![(
            "Permanent redirect".to_string(),
            "to: \"https://example.com/new\"\ncode: 301".to_string(),
        )],
    ));
    md.push_str(&render_strategy_ref_section());
    md.push_str(&render_middleware_config_section());

    md
}

fn render_examples_markdown(workspace_root: &Path) -> String {
    let global_example = read_optional_file(&workspace_root.join("httpward.yaml"));
    let strategies_example = read_optional_file(&workspace_root.join("strategies.yml"))
        .or_else(|| read_optional_file(&workspace_root.join("strategies.yaml")));
    let site_example = find_site_example(workspace_root);

    let mut md = String::new();
    md.push_str("---\n");
    md.push_str("title: \"Configuration Examples\"\n");
    md.push_str("description: \"Copy-pasteable YAML examples for HttpWard configuration.\"\n");
    md.push_str("keywords: [\"config\", \"yaml\", \"examples\", \"httpward\"]\n");
    md.push_str("---\n\n");
    md.push_str("# HttpWard Configuration Examples\n\n");
    md.push_str(
        "Use this page when you want ready-to-adapt YAML snippets. For the full field-by-field reference, go back to [Configuration](configuration.md).\n\n",
    );
    md.push_str("<a id=\"table-of-contents\"></a>\n\n");
    md.push_str("## Table of Contents\n\n");
    md.push_str("- [Minimal reverse proxy](#minimal-reverse-proxy)\n");
    md.push_str("- [Multi-site by domains (recommended)](#multi-site-by-domains-recommended)\n");
    md.push_str("- [TLS listener](#tls-listener)\n");
    md.push_str("- [Static files](#static-files)\n");
    md.push_str("- [Redirect](#redirect)\n");
    md.push_str("- [Inline route strategy](#inline-route-strategy)\n");
    md.push_str("- [Disable inherited middleware](#disable-inherited-middleware)\n");
    md.push_str("- [Examples from this repository](#examples-from-this-repository)\n\n");

    md.push_str(&render_section(
        "Minimal reverse proxy",
        &render_yaml_example(
            "`httpward.yaml`",
            r#"domain: example.com
listeners:
  - port: 80
strategy: "default"
sites_enabled: ./sites-enabled

routes:
  - match:
      path: "/"
    backend: "http://127.0.0.1:3000""#,
        ),
    ));
    md.push_str(&render_section(
        "Multi-site by domains (recommended)",
        &format!(
            "Use this pair of files to split traffic by domain. The key is `sites_enabled: \"./sites-enabled\"` in the global file.\n\n{}{}",
            render_yaml_example("`httpward.yaml`", multisite_httpward_yaml()),
            render_yaml_example("`sites-enabled/test.local.yml`", multisite_site_yaml()),
        ),
    ));
    md.push_str(&render_section(
        "TLS listener",
        &render_yaml_example(
            "HTTPS with self-signed certificate",
            r#"listeners:
  - port: 443
    tls:
      self_signed: true"#,
        ),
    ));
    md.push_str(&render_section(
        "Static files",
        &render_yaml_example(
            "Serve a directory",
            r#"routes:
  - match:
      path: "/assets/{*path}"
    static_dir: "C:/www/assets/{*path}""#,
        ),
    ));
    md.push_str(&render_section(
        "Redirect",
        &render_yaml_example(
            "Move one path permanently",
            r#"routes:
  - match:
      path: "/old"
    redirect:
      to: "https://example.com/new"
      code: 301"#,
        ),
    ));
    md.push_str(&render_section(
        "Inline route strategy",
        &render_yaml_example(
            "Inline middleware on a single route",
            r#"routes:
  - match:
      path: "/api/{*path}"
    backend: "http://127.0.0.1:8080/{*path}"
    strategy:
      - rate_limit:
          requests: 50
          window: "1m"
      - logging:
          level: info"#,
        ),
    ));
    md.push_str(&render_section(
        "Disable inherited middleware",
        &render_yaml_example(
            "Turn one middleware off locally",
            r#"strategy:
  - logging: off"#,
        ),
    ));

    let mut repo_examples = String::new();
    if let Some(example) = global_example {
        repo_examples.push_str(&render_yaml_example("Current `httpward.yaml`", &example));
    }
    if let Some(example) = strategies_example {
        repo_examples.push_str(&render_yaml_example("Current `strategies.yml`", &example));
    }
    if let Some((name, example)) = site_example {
        repo_examples.push_str(&render_yaml_example(
            &format!("Current `sites-enabled/{}`", name),
            &example,
        ));
    }
    if repo_examples.is_empty() {
        repo_examples.push_str("No repository examples were found next to the generator.\n");
    }
    md.push_str(&render_section(
        "Examples from this repository",
        &repo_examples,
    ));

    md
}

fn main() -> io::Result<()> {
    let schema = schema_for!(AppConfig);
    let json_schema = serde_json::to_string_pretty(&schema).expect("failed to serialize schema");
    let schema_value: Value =
        serde_json::from_str(&json_schema).expect("failed to parse JSON schema");

    let workspace_root = workspace_root();
    let docs_dir = workspace_root.join("docs");
    fs::create_dir_all(&docs_dir)?;

    let schema_path = docs_dir.join("config.schema.json");
    fs::write(&schema_path, &json_schema)?;
    println!("Schema successfully written -> {}", schema_path.display());

    let configuration_md = render_configuration_markdown(&schema_value, &workspace_root);
    let configuration_path = docs_dir.join("configuration/configuration.md");
    fs::write(&configuration_path, configuration_md)?;
    println!(
        "Configuration reference successfully written -> {}",
        configuration_path.display()
    );

    let examples_md = render_examples_markdown(&workspace_root);
    let examples_path = docs_dir.join("configuration/configuration-examples.md");
    fs::write(&examples_path, examples_md)?;
    println!(
        "Configuration examples successfully written -> {}",
        examples_path.display()
    );

    Ok(())
}
