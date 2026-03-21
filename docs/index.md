# HttpWard

A high-performance HTTP proxy and reverse proxy server built with Rust, designed for modern web applications and microservices architecture.

## Overview

HttpWard is a powerful, flexible, and efficient proxy server that provides:

- **High Performance**: Built with Rust for maximum speed and reliability
- **Flexible Configuration**: YAML-based configuration with advanced routing and middleware support
- **Modern Architecture**: Support for HTTP/1.1, HTTP/2, and WebSocket protocols
- **Extensible**: Plugin system for custom middleware and extensions
- **Production Ready**: Comprehensive logging, monitoring, and error handling

## Quick Start

### Installation

```bash
cargo install httpward
```

### Basic Configuration

Create a `httpward.yaml` file:

```yaml
# Global config (shared listeners/routes/strategies)
listeners:
  - port: 443
    tls:
      self_signed: true

sites_enabled: "./sites-enabled"

routes:
  - match:
      path: "/"
    backend: "http://127.0.0.1:3000"
```

Create `sites-enabled/test.local.yml`:

```yaml
domains: ["test.local", "*.test2.local"]

routes:
  - match:
      path: "/api"
    backend: "http://127.0.0.1:8080"
```

When you want domain-separated sites, keep each site in its own file under `sites-enabled/` and set `sites_enabled` in `httpward.yaml`.

### Running HttpWard

```bash
httpward --config httpward.yaml
```

## Key Features

### Advanced Routing
- Path-based routing with regex support
- Domain-based virtual hosting
- HTTP method matching
- Header and query parameter routing

### Middleware System
- Rate limiting
- Authentication and authorization
- CORS handling
- Request/response transformation
- Custom middleware support

### TLS/SSL Support
- Automatic certificate management
- SNI-based routing
- Custom certificate configuration

### Monitoring & Logging
- Structured logging with multiple levels
- Request tracing
- Performance metrics
- Health check endpoints

## Documentation

- **[Configuration Guide](configuration/configuration.md)** - Complete configuration reference
- **[Configuration Examples](configuration/configuration-examples.md)** - Practical examples and use cases

## Community

- **GitHub**: [https://github.com/ZEREX222/HttpWard](https://github.com/ZEREX222/HttpWard)
- **Issues**: Report bugs and request features
- **Discussions**: Community discussions and Q&A

## License

HttpWard is licensed under the MIT License. See [LICENSE](https://github.com/ZEREX222/HttpWard/blob/main/LICENSE) for details.

