use rama::{
    layer::Layer,
    service::Service,
    http::{Body, Request, Response},
};

/// A simple and practical middleware pipeline builder
/// 
/// This provides a convenient way to build middleware chains without nested .layer() calls
/// 
/// # Example:
/// ```rust
/// use httpward_core::middleware::{EnricherLayer, LogLayer, MiddlewarePipe};
/// use rama::service::service_fn;
/// 
/// let pipeline = MiddlewarePipe::new()
///     .add_enricher()
///     .add_log();
/// 
/// let service = pipeline.build(service_fn(handler));
/// ```
#[derive(Clone, Debug)]
pub struct MiddlewarePipe;

impl MiddlewarePipe {
    /// Create a new empty middleware pipeline
    pub fn new() -> Self {
        Self
    }

    /// Add an EnricherLayer to the pipeline
    pub fn add_enricher(self) -> MiddlewarePipeWithEnricher {
        MiddlewarePipeWithEnricher { pipe: self }
    }

    /// Add a LogLayer to the pipeline
    pub fn add_log(self) -> MiddlewarePipeWithLog {
        MiddlewarePipeWithLog { pipe: self }
    }
}

/// Pipeline with EnricherLayer
#[derive(Clone, Debug)]
pub struct MiddlewarePipeWithEnricher {
    pipe: MiddlewarePipe,
}

impl MiddlewarePipeWithEnricher {
    /// Add a LogLayer to the pipeline (Enricher -> Log)
    pub fn add_log(self) -> MiddlewarePipeWithEnricherLog {
        MiddlewarePipeWithEnricherLog { pipe: self }
    }

    /// Build the final service with only EnricherLayer
    pub fn build<S>(self, service: S) -> crate::middleware::EnricherService<S>
    where
        S: Service<(), Request<Body>>,
    {
        crate::middleware::EnricherLayer::new().layer(service)
    }
}

/// Pipeline with LogLayer
#[derive(Clone, Debug)]
pub struct MiddlewarePipeWithLog {
    pipe: MiddlewarePipe,
}

impl MiddlewarePipeWithLog {
    /// Add an EnricherLayer to the pipeline (Log -> Enricher)
    pub fn add_enricher(self) -> MiddlewarePipeWithLogEnricher {
        MiddlewarePipeWithLogEnricher { pipe: self }
    }

    /// Build the final service with only LogLayer
    pub fn build<S>(self, service: S) -> crate::middleware::LogService<S>
    where
        S: Service<(), Request<Body>>,
    {
        crate::middleware::LogLayer::new().layer(service)
    }
}

/// Pipeline with EnricherLayer -> LogLayer
#[derive(Clone, Debug)]
pub struct MiddlewarePipeWithEnricherLog {
    pipe: MiddlewarePipeWithEnricher,
}

impl MiddlewarePipeWithEnricherLog {
    /// Build the final service with Enricher -> Log layers
    pub fn build<S>(self, service: S) -> crate::middleware::LogService<crate::middleware::EnricherService<S>>
    where
        S: Service<(), Request<Body>>,
    {
        let enriched = crate::middleware::EnricherLayer::new().layer(service);
        crate::middleware::LogLayer::new().layer(enriched)
    }
}

/// Pipeline with LogLayer -> EnricherLayer
#[derive(Clone, Debug)]
pub struct MiddlewarePipeWithLogEnricher {
    pipe: MiddlewarePipeWithLog,
}

impl MiddlewarePipeWithLogEnricher {
    /// Build the final service with Log -> Enricher layers
    pub fn build<S>(self, service: S) -> crate::middleware::EnricherService<crate::middleware::LogService<S>>
    where
        S: Service<(), Request<Body>>,
    {
        let logged = crate::middleware::LogLayer::new().layer(service);
        crate::middleware::EnricherLayer::new().layer(logged)
    }
}

/// Pre-built pipelines for common use cases
pub struct PrebuiltPipelines;

impl PrebuiltPipelines {
    /// Standard pipeline: Enricher -> Log
    pub fn standard<S>(service: S) -> crate::middleware::LogService<crate::middleware::EnricherService<S>>
    where
        S: Service<(), Request<Body>>,
    {
        MiddlewarePipe::new()
            .add_enricher()
            .add_log()
            .build(service)
    }

    /// Logging only pipeline
    pub fn log_only<S>(service: S) -> crate::middleware::LogService<S>
    where
        S: Service<(), Request<Body>>,
    {
        MiddlewarePipe::new()
            .add_log()
            .build(service)
    }

    /// Enrichment only pipeline
    pub fn enrich_only<S>(service: S) -> crate::middleware::EnricherService<S>
    where
        S: Service<(), Request<Body>>,
    {
        MiddlewarePipe::new()
            .add_enricher()
            .build(service)
    }
}

/// Macro for building pipelines with custom layer combinations
/// 
/// # Example:
/// ```rust
/// use httpward_core::{build_pipeline};
/// use httpward_core::middleware::{EnricherLayer, LogLayer};
/// 
/// let service = build_pipeline!(
///     base_service,
///     EnricherLayer::new(),
///     LogLayer::new(),
/// );
/// ```
#[macro_export]
macro_rules! build_pipeline {
    ($service:expr, $($layer:expr),+ $(,)?) => {{
        let mut result = $service;
        $(
            result = $layer.layer(result);
        )+
        result
    }};
}
