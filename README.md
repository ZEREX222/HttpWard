![hw.png](resources/hw.png)

# HttpWard

HttpWard is a lightweight, high-performance L7 reverse proxy written in Rust, designed for modern web applications and microservices architecture. It provides strong security features, intelligent routing, flexible middleware system, and extremely low resource usage.

## Features

### Core Capabilities
- **High Performance**: Built with Rust for maximum speed and reliability
- **Protocol Support**: HTTP/1.1, HTTP/2, and WebSocket protocols
- **TLS/SSL Support**: Automatic certificate management with SNI-based routing
- **Flexible Configuration**: YAML-based configuration with advanced routing

### Advanced Routing
- **Path-based Routing**: Regex support for complex path patterns
- **Domain-based Virtual Hosting**: Multiple domains with per-site configuration
- **HTTP Method Matching**: Route based on GET, POST, PUT, DELETE, etc.
- **Header & Query Routing**: Route based on request headers and query parameters

### Security & Protection
- **Web Application Firewall (WAF)**: Request filtering and validation
- **Rate Limiting**: Configurable request rate limits per route or globally
- **DDoS Mitigation**: Built-in protection against common attack vectors
- **Authentication & Authorization**: JWT and custom auth middleware support

### Middleware System
- **Strategy-based Configuration**: Hierarchical middleware strategies (global → site → route)
- **Dynamic Module Loading**: Runtime loading of custom middleware modules
- **Built-in Middleware**: Rate limiting, logging, CORS, authentication, and more
- **Custom Middleware**: Easy development of custom middleware extensions

### Monitoring & Operations
- **Structured Logging**: Comprehensive request/response logging with multiple levels
- **Performance Metrics**: Built-in monitoring and health check endpoints
- **Error Handling**: Professional HTML error pages with proper status codes
- **Hot Configuration Reload**: Runtime configuration updates without service restart

## Quick Start

### Installation

```bash
cargo install httpward
```

### Basic Configuration

Create a `httpward.yaml` file:

```yaml
listeners:
  - port: 443
    tls:
      self_signed: true

sites_enabled: "./sites-enabled"

routes:
  - match:
      path: "/{*path}"
    backend: "http://127.0.0.1:3000/{*path}"
```

Create `sites-enabled/test.local.yml`:

```yaml
domains: ["test.local", "*.test2.local"]

routes:
  - match:
      path: "/api/{*path}"
    backend: "http://127.0.0.1:8080/{*path}"
```

If you want to separate sites by domain, keep one config file per site inside `sites-enabled/` and point `sites_enabled` to that folder.

### Advanced Configuration with Strategies

```yaml
# Global strategy
strategy: "default"
strategies:
  default:
    - rate_limit:
        requests: 1000
        window: "1m"
    - logging:
        level: info
```

```yaml
# sites-enabled/api.example.com.yml
domain: "api.example.com"

strategy: "api_strict"  # Override global strategy
strategies:
  api_strict:
    - rate_limit:
        requests: 100
        window: "1m"
    - auth:
        type: jwt
    - logging:
        level: debug

routes:
  - match:
      path: "/api/v1/{*path}"
    backend: "http://api-service:8080/{*path}"
```

### Running HttpWard

```bash
httpward --config httpward.yaml
```

## Documentation

📖 **[Complete Documentation](https://zerex222.github.io/HttpWard)**

- **[Configuration Guide](https://zerex222.github.io/HttpWard/configuration/configuration/)** - Complete configuration reference
- **[Configuration Examples](https://zerex222.github.io/HttpWard/configuration/configuration-examples/)** - Practical examples and use cases
- **[Extensions Guide](https://zerex222.github.io/HttpWard/guides/extensions-guide/)** - Building and using extensions
- **[Extensions Migration Guide](https://zerex222.github.io/HttpWard/guides/extensions-migration-guide/)** - Migrating between extension versions

## Architecture

HttpWard is built with a modular architecture:

- **httpward-core**: Core library with configuration, routing, and middleware systems
- **httpward**: Main binary application with server implementation
- **httpward-modules**: Extensible module system for custom middleware
- **httpward-docs**: Documentation and examples

## Performance

- **Memory Usage**: Optimized for minimal memory footprint
- **CPU Efficiency**: Async I/O with zero-copy where possible
- **Scalability**: Handles thousands of concurrent connections
- **Low Latency**: Sub-millisecond proxy overhead

## Community

- **GitHub**: [https://github.com/ZEREX222/HttpWard](https://github.com/ZEREX222/HttpWard)
- **Issues**: Report bugs and request features
- **Discussions**: Community discussions and Q&A

## License

HttpWard is licensed under the MPL-2.0 License. See [LICENSE](LICENSE) for details.
