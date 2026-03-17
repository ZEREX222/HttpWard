---
title: "Configuration"
description: "Human-friendly guide to HttpWard configuration files."
keywords: ["config", "yaml", "schema", "httpward"]
---

# HttpWard Configuration

> This page explains the files that people actually edit: `httpward.yaml`, `strategies.yml`, and `sites-enabled/*.yml`. HttpWard loads those files into an internal [`AppConfig`](#appconfig), but the focus here is practical YAML, not raw schema internals.

For a page with copy-pasteable recipes, see [Configuration examples](configuration-examples.md).

<a id="table-of-contents"></a>

## Table of Contents

- [How configuration is loaded](#how-configuration-is-loaded)
- [Quick start](#quick-start)
- [AppConfig](#appconfig)
- [Global file: `httpward.yaml`](#global-file-httpward-yaml)
- [Site files: `sites-enabled/*.yml`](#site-files-sites-enabled-yml)
- [Strategies file: `strategies.yml`](#strategies-file-strategies-yml)
- [Reusable types](#reusable-types)
  - [`Listener`](#listener)
  - [`Tls`](#tls)
  - [`LogConfig`](#logconfig)
  - [`Match`](#match)
  - [`Route`](#route)
  - [`Redirect`](#redirect)
  - [`StrategyRef`](#strategyref)
  - [`MiddlewareConfig`](#middlewareconfig)

<a id="how-configuration-is-loaded"></a>

## How configuration is loaded

1. HttpWard reads `httpward.yaml` as the global configuration.
2. If `strategies.yml` exists next to it, those named strategies are loaded and merged in.
3. HttpWard then reads all `*.yml` / `*.yaml` files from the directory referenced by `sites_enabled`.
4. At runtime those pieces become a single `AppConfig { global, sites }`.

Important validation rules:

- a site file must define either `domain` or `domains`;
- a listener with `tls` enabled must use a non-zero `port`;
- `strategy` can be a string name or an inline middleware list.

<a id="quick-start"></a>

## Quick start

Start with these three files:

### Minimal `httpward.yaml`

```yaml
domain: example.com
listeners:
  - port: 80
sites_enabled: ./sites-enabled
```

### Minimal `strategies.yml`

```yaml
default:
  - logging:
      level: info
  - rate_limit:
      requests: 1000
      window: "1m"
```

### Minimal site file

```yaml
domain: app.example.com
routes:
  - match:
      path: "/"
    backend: "http://127.0.0.1:3000"
```

<a id="appconfig"></a>

## AppConfig

`AppConfig` is the combined in-memory model, not a file you write by hand. It is useful for tooling, validation, and the generated `config.schema.json`.

| Field | Type | Description |
|---|---|---|
| `global` | [`GlobalConfig`](#globalconfig) | Parsed from `httpward.yaml` |
| `sites` | list of [`SiteConfig`](#siteconfig) | Parsed from the directory configured in `sites_enabled` |

The JSON Schema generated from this type is written to `docs/config.schema.json`.

<a id="globalconfig"></a>

## `GlobalConfig`

`httpward.yaml` is the main file in the project root. It defines listeners, default routing, logging, the site directory, and optional default strategies.

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `domain` | `string` | no | `""` | Primary domain name (used for SNI matching & logging) |
| `domains` | list of `string` | no | see examples | Additional domain names / aliases |
| `listeners` | list of [`Listener`](#listener) | no | see examples | Network listeners (bind address + port + optional TLS) |
| `routes` | list of [`Route`](#route) | no | see examples | Global routing rules (executed before site-level routes) |
| `log` | [`LogConfig`](#logconfig) | no | see examples | Logging configuration |
| `sites_enabled` | `string` | no | `""` | Path to directory with per-site .yaml / .yml files |
| `strategy` | optional [`StrategyRef`](#strategyref) | no | `"default"` | Default strategy for all domains and routes |
| `strategies` | map of string to list of [`MiddlewareConfig`](#middlewareconfig) | no | see examples | Global strategy definitions |

### Recommended starting point

```yaml
domain: example.com
listeners:
  - port: 80
sites_enabled: ./sites-enabled
```

### Example from this repository

```yaml
# httpward.yaml - Global settings (applied to all sites unless overridden)

log:
  level: "debug"

domain: global.local

strategy: "default"

listeners:
  - port: 444
    tls:
      self_signed: true

routes:
  - match:
      path: "/my/{*any}"
    backend: "http://zerex222.ru:8080/{*any}"
    strategy:
      - block_ip:
          ips: [ "127.0.0.1"]

  - match:
      path: "/site/{*path}"
    static_dir: "C:/myprojects/html/{*path}"

  - match:
      path: "/redirect"
    redirect:
      to: "https://google.com"

sites_enabled: "./sites-enabled"
```

[↑ Back to top](#table-of-contents)

<a id="global-file-httpward-yaml"></a>

## Global file: `httpward.yaml`

This section documents the fields of [`GlobalConfig`](#globalconfig).

[↑ Back to top](#table-of-contents)

<a id="site-files-sites-enabled-yml"></a>

## Site files: `sites-enabled/*.yml`

Each file in `sites-enabled/` describes one site or virtual host. Site settings can override global listeners, routes, and strategies when needed.

<a id="siteconfig"></a>

## `SiteConfig`

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `domain` | `string` | no | `""` | Primary domain name (used for SNI matching & logging) |
| `domains` | list of `string` | no | see examples | Additional domain names / aliases |
| `listeners` | list of [`Listener`](#listener) | no | see examples | Optional site-specific listeners (overrides global listeners) |
| `routes` | list of [`Route`](#route) | no | see examples | Site-level routing rules |
| `strategy` | optional [`StrategyRef`](#strategyref) | no | `null` | Site-specific strategy (overrides global default) |
| `strategies` | map of string to list of [`MiddlewareConfig`](#middlewareconfig) | no | see examples | Site-specific strategy definitions |

### Minimal site file

```yaml
domain: app.example.com
routes:
  - match:
      path: "/"
    backend: "http://127.0.0.1:3000"
```

### Example from this repository: `sites-enabled/test.local.yml`

```yaml
domains: ["test.local", "*.test2.local"]

listeners:
  - port: 777
  - port: 443
    tls:
      self_signed: true

routes:
  - match:
      path: "/api"
    backend: "http://127.0.0.1:8080"

  - match:
      path: "/static"
    static_dir: "/var/www/example.com/static"

  - match:
      path: "/aaa/{id}"
    backend: "http://127.0.0.1:3000/api/{id}"
```

[↑ Back to top](#table-of-contents)

<a id="strategies-file-strategies-yml"></a>

## Strategies file: `strategies.yml`

`strategies.yml` is a map of strategy name to an array of middleware entries. It is the best place for reusable policies such as logging, rate limiting, auth, or headers.

Structure rules:

- the top-level key is the strategy name;
- the value is a YAML list;
- each list item is a [`MiddlewareConfig`](#middlewareconfig);
- you can reference a strategy by name with [`StrategyRef`](#strategyref).

### Reusable strategies

```yaml
# HttpWard Strategies Configuration
# This file defines reusable middleware strategies that can be applied at different levels

# Default strategy applied globally
default:
  - rate_limit:
      requests: 1000
      window: "1m"
  - logging:
      level: info

# For super safe mode
super-safe:
  - rate_limit:
      requests: 10
      window: "1m"
  - logging:
      level: info
```

### Disable one inherited middleware

```yaml
safe-mode:
  - rate_limit:
      requests: 10
      window: "1m"
  - logging: off
```

[↑ Back to top](#table-of-contents)

<a id="reusable-types"></a>

## Reusable types

The following sections document the nested building blocks reused across global config, site config, routes, listeners, and strategies.

<a id="listener"></a>

## `Listener`

A listener binds HttpWard to an address/port and can optionally enable TLS.

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `host` | `string` | no | `"0.0.0.0"` | Bind address (default: 0.0.0.0) |
| `port` | `integer` | no | `0` | TCP port |
| `tls` | optional [`Tls`](#tls) | no | `null` | Optional TLS configuration |

### HTTP listener

```yaml
host: "0.0.0.0"
port: 80
```

### HTTPS listener with self-signed certificate

```yaml
host: "0.0.0.0"
port: 443
tls:
  self_signed: true
```

[↑ Back to top](#table-of-contents)

<a id="tls"></a>

## `Tls`

TLS settings for a listener. Use `self_signed: true` for local development; in production prefer explicit `cert` and `key` paths.

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `self_signed` | `boolean` | no | `false` | — |
| `cert` | `string` | no | `""` | — |
| `key` | `string` | no | `""` | — |

### Development TLS

```yaml
self_signed: true
```

### Certificate files

```yaml
self_signed: false
cert: "C:/certs/fullchain.pem"
key: "C:/certs/privkey.pem"
```

[↑ Back to top](#table-of-contents)

<a id="logconfig"></a>

## `LogConfig`

Logging settings used by the built-in logging module and related middleware.

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `level` | `string` | no | `"warn"` | Logging level ("trace" \| "debug" \| "info" \| "warn" \| "error") |

### Recommended default

```yaml
level: warn
```

[↑ Back to top](#table-of-contents)

<a id="match"></a>

## `Match`

A path matcher used by routes. Prefer `path` for readability and performance. Reach for `path_regex` only when pattern routing cannot be expressed with path templates.

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `path` | optional `string` | no | `null` | Using matchit library https://github.com/ibraheemdev/matchit |
| `path_regex` | optional `string` | no | `null` | Using basic regexp. Please use path if it's possible. |

### Template path

```yaml
path: "/api/{*path}"
```

### Regex path

```yaml
path_regex: "^/v[0-9]+/api"
```

[↑ Back to top](#table-of-contents)

<a id="route"></a>

## `Route`

A route decides what HttpWard should do with a matching request. In practice there are three forms: proxy to an upstream, serve static files, or return a redirect.

Common fields:

- `match` — path matcher. Prefer `path` when possible; use `path_regex` only when you really need regex behavior.
- `strategy` — either a named strategy like `"default"` or an inline list of middleware.
- `strategies` — per-route named strategy map if you want route-local reusable strategies.

### Proxy route

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `match` | [`Match`](#match) | yes | see examples | — |
| `backend` | `string` | yes | — | — |
| `strategy` | optional [`StrategyRef`](#strategyref) | no | `null` | — |
| `strategies` | optional `object` | no | `null` | — |

### Example

```yaml
match:
  path: "/api/{*path}"
backend: "http://127.0.0.1:8080/{*path}"
strategy: "default"
```

### Static files route

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `match` | [`Match`](#match) | yes | see examples | — |
| `static_dir` | `string` | yes | — | — |
| `strategy` | optional [`StrategyRef`](#strategyref) | no | `null` | — |
| `strategies` | optional `object` | no | `null` | — |

### Example

```yaml
match:
  path: "/assets/{*path}"
static_dir: "C:/sites/example/assets/{*path}"
```

### Redirect route

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `match` | [`Match`](#match) | yes | see examples | — |
| `redirect` | [`Redirect`](#redirect) | yes | see examples | — |
| `strategy` | optional [`StrategyRef`](#strategyref) | no | `null` | — |
| `strategies` | optional `object` | no | `null` | — |

### Example

```yaml
match:
  path: "/old"
redirect:
  to: "https://example.com/new"
  code: 301
```

[↑ Back to top](#table-of-contents)

<a id="redirect"></a>

## `Redirect`

Redirect target used by redirect routes.

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `to` | `string` | yes | — | — |
| `code` | `integer` | no | `301` | — |

### Permanent redirect

```yaml
to: "https://example.com/new"
code: 301
```

[↑ Back to top](#table-of-contents)

<a id="strategyref"></a>

## `StrategyRef`

A strategy reference can be written in two user-friendly ways.

1. **Named strategy** — points to a strategy defined in `strategies.yml` or in a local `strategies:` map.
2. **Inline middleware list** — define middleware directly where the strategy is used.

### Named strategy

```yaml
strategy: "default"
```

### Inline strategy

```yaml
strategy:
  - rate_limit:
      requests: 100
      window: "1m"
  - logging:
      level: info
```

[↑ Back to top](#table-of-contents)

<a id="middlewareconfig"></a>

## `MiddlewareConfig`

Each middleware item in a strategy is a single-key YAML object. The key is the middleware name; the value is either its configuration, `off`, or `false`.

### Enabled middleware

```yaml
- logging:
    level: info
```

### Disable middleware with `off`

```yaml
- logging: off
```

### Disable middleware with `false`

```yaml
- logging: false
```

This is especially useful when a site or route inherits a strategy from above and you want to turn one middleware off locally.

[↑ Back to top](#table-of-contents)

