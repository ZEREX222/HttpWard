---
title: "Configuration"
description: "AppConfig — Combined configuration (global + sites)."
keywords: ["config", "schema", "httpward"]
---

# AppConfig — Configuration

> Combined configuration in memory: global + all loaded sites

## Table of Contents

- [Top-level structure](#top-level-structure)
- [`GlobalConfig`](#globalconfig)
- [`Listener`](#listener)
- [`LogConfig`](#logconfig)
- [`Match`](#match)
- [`MiddlewareConfig`](#middlewareconfig)
- [`Redirect`](#redirect)
- [`Route`](#route)
- [`SiteConfig`](#siteconfig)
- [`StrategyRef`](#strategyref)
- [`Tls`](#tls)

---

## Top-level structure

- **`global`** — ['GlobalConfig'](#globalconfig)
- **`sites`** — array of ['SiteConfig'](#siteconfig)

---

## Definitions

<a id="globalconfig"></a>

## `GlobalConfig`

Global application configuration (loaded from httpward.yaml)<br>Inherits all fields from SiteConfig plus global-specific settings

| Field | Type | Description | Default | Required |
|---|---|---|---|---|
| `domain` | `string` | Primary domain name (used for SNI matching & logging) | `""` | no |
| `domains` | `array of string` | Additional domain names / aliases | `["..."]` | no |
| `listeners` | `array of ['Listener'](#listener)` | Network listeners (bind address + port + optional TLS) | `   host: "0.0.0.0"   port: 0   tls:    self_signed: false    cert: ""    key: ""     # other options: Type null ` | no |
| `routes` | `array of ['Route'](#route)` | Global routing rules (executed before site-level routes) | `   match:    path: null    path_regex: null   backend: "..."   strategy:    "..." # other options: Array of ['MiddlewareConfig'](#middlewareconfig)     # other options: Type null   strategies: null    # other options: Static Files, Redirect ` | no |
| `log` | `['LogConfig'](#logconfig)` | Logging configuration | `level: "warn" ` | no |
| `sites_enabled` | `string` | Path to directory with per-site .yaml / .yml files | `""` | no |
| `strategy` | `(['StrategyRef'](#strategyref) | null)` | Default strategy for all domains and routes | `"..." # other options: Array of ['MiddlewareConfig'](#middlewareconfig)  # other options: Type null ` | no |
| `strategies` | `object` | Global strategy definitions | `<key>:    Named:     name: "..."     config: "..."     # other options: Off ` | no |

Example (YAML):

```yaml
domain: ""
domains: ["..."]
listeners:
  
      host: "0.0.0.0"
      port: 0
      tls:
        self_signed: false
        cert: ""
        key: ""
          # other options: Type null
routes:
  
      match:
        path: null
        path_regex: null
      backend: "..."
      strategy:
        "..."  # other options: Array of [`MiddlewareConfig`](#middlewareconfig)
          # other options: Type null
      strategies: null
        # other options: Static Files, Redirect
log:
  level: "warn"
sites_enabled: ""
strategy:
  "..."  # other options: Array of [`MiddlewareConfig`](#middlewareconfig)
    # other options: Type null
strategies:
  <key>: 
        Named:
          name: "..."
          config: "..."
          # other options: Off
```

**References:** [`Listener`](#listener), [`LogConfig`](#logconfig), [`MiddlewareConfig`](#middlewareconfig), [`Route`](#route), [`StrategyRef`](#strategyref)

[↑ Back to top](#table-of-contents)

<a id="listener"></a>

## `Listener`

| Field | Type | Description | Default | Required |
|---|---|---|---|---|
| `host` | `string` | Bind address (default: 0.0.0.0) | `"0.0.0.0"` | no |
| `port` | `integer` | TCP port | `0` | no |
| `tls` | `(['Tls'](#tls) | null)` | Optional TLS configuration | `self_signed: false cert: "" key: ""  # other options: Type null ` | no |

Example (YAML):

```yaml
host: "0.0.0.0"
port: 0
tls:
  self_signed: false
  cert: ""
  key: ""
    # other options: Type null
```

**References:** [`Tls`](#tls)

[↑ Back to top](#table-of-contents)

<a id="logconfig"></a>

## `LogConfig`

| Field | Type | Description | Default | Required |
|---|---|---|---|---|
| `level` | `string` | Logging level ("trace" \| "debug" \| "info" \| "warn" \| "error") | `"warn"` | no |

Example (YAML):

```yaml
level: "warn"
```

[↑ Back to top](#table-of-contents)

<a id="match"></a>

## `Match`

| Field | Type | Description | Default | Required |
|---|---|---|---|---|
| `path` | `string|null` | Using matchit library https://github.com/ibraheemdev/matchit | `null` | no |
| `path_regex` | `string|null` | Using basic regexp. Please use path if it's possible. | `null` | no |

Example (YAML):

```yaml
path: null
path_regex: null
```

[↑ Back to top](#table-of-contents)

<a id="middlewareconfig"></a>

## `MiddlewareConfig`

### Variants

#### Named

<a id="middlewareconfig - named"></a>

## `MiddlewareConfig - Named`

| Field | Type | Description | Default | Required |
|---|---|---|---|---|
| `Named` | `object` |  | `{}` | yes |

Example (YAML):

```yaml
Named:
  name: "..."
  config: "..."
```

[↑ Back to top](#table-of-contents)

#### Off

<a id="middlewareconfig - off"></a>

## `MiddlewareConfig - Off`

| Field | Type | Description | Default | Required |
|---|---|---|---|---|
| `Off` | `object` |  | `{}` | yes |

Example (YAML):

```yaml
Off:
  name: "..."
```

[↑ Back to top](#table-of-contents)

[↑ Back to top](#table-of-contents)

<a id="redirect"></a>

## `Redirect`

| Field | Type | Description | Default | Required |
|---|---|---|---|---|
| `to` | `string` |  | `"..."` | yes |
| `code` | `integer` |  | `301` | no |

Example (YAML):

```yaml
to: "..."
code: 301
```

[↑ Back to top](#table-of-contents)

<a id="route"></a>

## `Route`

Single routing rule — proxy / static / redirect

### Variants

#### Proxy

<a id="route - proxy"></a>

## `Route - Proxy`

| Field | Type | Description | Default | Required |
|---|---|---|---|---|
| `match` | `['Match'](#match)` |  | `# see Match` | yes |
| `backend` | `string` |  | `"..."` | yes |
| `strategy` | `(['StrategyRef'](#strategyref) | null)` |  | `"..." # other options: Array of ['MiddlewareConfig'](#middlewareconfig)  # other options: Type null ` | no |
| `strategies` | `object|null` |  | `null` | no |

Example (YAML):

```yaml
match:
  path: null
  path_regex: null
backend: "..."
strategy:
  "..."  # other options: Array of [`MiddlewareConfig`](#middlewareconfig)
    # other options: Type null
strategies: null
```

**References:** [`Match`](#match), [`MiddlewareConfig`](#middlewareconfig), [`StrategyRef`](#strategyref)

[↑ Back to top](#table-of-contents)

#### Static Files

<a id="route - static files"></a>

## `Route - Static Files`

| Field | Type | Description | Default | Required |
|---|---|---|---|---|
| `match` | `['Match'](#match)` |  | `# see Match` | yes |
| `static_dir` | `string` |  | `"..."` | yes |
| `strategy` | `(['StrategyRef'](#strategyref) | null)` |  | `"..." # other options: Array of ['MiddlewareConfig'](#middlewareconfig)  # other options: Type null ` | no |
| `strategies` | `object|null` |  | `null` | no |

Example (YAML):

```yaml
match:
  path: null
  path_regex: null
static_dir: "..."
strategy:
  "..."  # other options: Array of [`MiddlewareConfig`](#middlewareconfig)
    # other options: Type null
strategies: null
```

**References:** [`Match`](#match), [`MiddlewareConfig`](#middlewareconfig), [`StrategyRef`](#strategyref)

[↑ Back to top](#table-of-contents)

#### Redirect

<a id="route - redirect"></a>

## `Route - Redirect`

| Field | Type | Description | Default | Required |
|---|---|---|---|---|
| `match` | `['Match'](#match)` |  | `# see Match` | yes |
| `redirect` | `['Redirect'](#redirect)` |  | `# see Redirect` | yes |
| `strategy` | `(['StrategyRef'](#strategyref) | null)` |  | `"..." # other options: Array of ['MiddlewareConfig'](#middlewareconfig)  # other options: Type null ` | no |
| `strategies` | `object|null` |  | `null` | no |

Example (YAML):

```yaml
match:
  path: null
  path_regex: null
redirect:
  to: "..."
  code: 301
strategy:
  "..."  # other options: Array of [`MiddlewareConfig`](#middlewareconfig)
    # other options: Type null
strategies: null
```

**References:** [`Match`](#match), [`MiddlewareConfig`](#middlewareconfig), [`Redirect`](#redirect), [`StrategyRef`](#strategyref)

[↑ Back to top](#table-of-contents)

**References:** [`Match`](#match), [`MiddlewareConfig`](#middlewareconfig), [`Redirect`](#redirect), [`StrategyRef`](#strategyref)

[↑ Back to top](#table-of-contents)

<a id="siteconfig"></a>

## `SiteConfig`

Configuration for one virtual host / site

| Field | Type | Description | Default | Required |
|---|---|---|---|---|
| `domain` | `string` | Primary domain name (used for SNI matching & logging) | `""` | no |
| `domains` | `array of string` | Additional domain names / aliases | `["..."]` | no |
| `listeners` | `array of ['Listener'](#listener)` | Optional site-specific listeners (overrides global listeners) | `   host: "0.0.0.0"   port: 0   tls:    self_signed: false    cert: ""    key: ""     # other options: Type null ` | no |
| `routes` | `array of ['Route'](#route)` | Site-level routing rules | `   match:    path: null    path_regex: null   backend: "..."   strategy:    "..." # other options: Array of ['MiddlewareConfig'](#middlewareconfig)     # other options: Type null   strategies: null    # other options: Static Files, Redirect ` | no |
| `strategy` | `(['StrategyRef'](#strategyref) | null)` | Site-specific strategy (overrides global default) | `"..." # other options: Array of ['MiddlewareConfig'](#middlewareconfig)  # other options: Type null ` | no |
| `strategies` | `object` | Site-specific strategy definitions | `<key>:    Named:     name: "..."     config: "..."     # other options: Off ` | no |

Example (YAML):

```yaml
domain: ""
domains: ["..."]
listeners:
  
      host: "0.0.0.0"
      port: 0
      tls:
        self_signed: false
        cert: ""
        key: ""
          # other options: Type null
routes:
  
      match:
        path: null
        path_regex: null
      backend: "..."
      strategy:
        "..."  # other options: Array of [`MiddlewareConfig`](#middlewareconfig)
          # other options: Type null
      strategies: null
        # other options: Static Files, Redirect
strategy:
  "..."  # other options: Array of [`MiddlewareConfig`](#middlewareconfig)
    # other options: Type null
strategies:
  <key>: 
        Named:
          name: "..."
          config: "..."
          # other options: Off
```

**References:** [`Listener`](#listener), [`MiddlewareConfig`](#middlewareconfig), [`Route`](#route), [`StrategyRef`](#strategyref)

[↑ Back to top](#table-of-contents)

<a id="strategyref"></a>

## `StrategyRef`

### Variants

#### String

- `string`

#### Array of [`MiddlewareConfig`](#middlewareconfig)

- `array of ['MiddlewareConfig'](#middlewareconfig)`

**References:** [`MiddlewareConfig`](#middlewareconfig)

[↑ Back to top](#table-of-contents)

<a id="tls"></a>

## `Tls`

| Field | Type | Description | Default | Required |
|---|---|---|---|---|
| `self_signed` | `boolean` |  | `false` | no |
| `cert` | `string` |  | `""` | no |
| `key` | `string` |  | `""` | no |

Example (YAML):

```yaml
self_signed: false
cert: ""
key: ""
```

[↑ Back to top](#table-of-contents)

