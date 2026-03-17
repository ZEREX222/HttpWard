---
title: "Configuration Examples"
description: "Copy-pasteable YAML examples for HttpWard configuration."
keywords: ["config", "yaml", "examples", "httpward"]
---

# HttpWard Configuration Examples

Use this page when you want ready-to-adapt YAML snippets. For the full field-by-field reference, go back to [Configuration](configuration.md).

<a id="table-of-contents"></a>

## Table of Contents

- [Minimal reverse proxy](#minimal-reverse-proxy)
- [TLS listener](#tls-listener)
- [Static files](#static-files)
- [Redirect](#redirect)
- [Inline route strategy](#inline-route-strategy)
- [Disable inherited middleware](#disable-inherited-middleware)
- [Examples from this repository](#examples-from-this-repository)

<a id="minimal-reverse-proxy"></a>

## Minimal reverse proxy

### `httpward.yaml`

```yaml
domain: example.com
listeners:
  - port: 80
strategy: "default"
sites_enabled: ./sites-enabled

routes:
  - match:
      path: "/"
    backend: "http://127.0.0.1:3000"
```

<a id="tls-listener"></a>

## TLS listener

### HTTPS with self-signed certificate

```yaml
listeners:
  - port: 443
    tls:
      self_signed: true
```

<a id="static-files"></a>

## Static files

### Serve a directory

```yaml
routes:
  - match:
      path: "/assets/{*path}"
    static_dir: "C:/www/assets/{*path}"
```

<a id="redirect"></a>

## Redirect

### Move one path permanently

```yaml
routes:
  - match:
      path: "/old"
    redirect:
      to: "https://example.com/new"
      code: 301
```

<a id="inline-route-strategy"></a>

## Inline route strategy

### Inline middleware on a single route

```yaml
routes:
  - match:
      path: "/api/{*path}"
    backend: "http://127.0.0.1:8080/{*path}"
    strategy:
      - rate_limit:
          requests: 50
          window: "1m"
      - logging:
          level: info
```

<a id="disable-inherited-middleware"></a>

## Disable inherited middleware

### Turn one middleware off locally

```yaml
strategy:
  - logging: off
```

<a id="examples-from-this-repository"></a>

## Examples from this repository

### Current `httpward.yaml`

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

### Current `strategies.yml`

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

### Current `sites-enabled/test.local.yml`

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

