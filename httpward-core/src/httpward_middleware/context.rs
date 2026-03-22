use crate::core::HttpWardContext;
use rama::Context;
use rama::net::address::SocketAddress;
use rama::net::tls::SecureTransport;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;

/// Host-owned request middleware context.
///
/// This context is created by the host per request and passed through the entire
/// middleware chain (static and dynamic). It snapshots frequently used Rama
/// extension values so dynamic DLL modules do not depend on cross-binary TypeId
/// lookups for common data.
#[derive(Debug, Clone)]
pub struct HttpwardMiddlewareContext {
    rama_context: Option<Context<()>>,
    httpward_context: Option<HttpWardContext>,
    secure_transport: Option<SecureTransport>,
    socket_address: Option<SocketAddress>,
    std_socket_addr: Option<SocketAddr>,
    shared: HashMap<String, Value>,
}

impl HttpwardMiddlewareContext {
    /// Create middleware context from incoming Rama context.
    pub fn from_rama_context(ctx: Context<()>) -> Self {
        let httpward_context = ctx.get::<HttpWardContext>().cloned();
        let secure_transport = ctx.get::<SecureTransport>().cloned();
        let socket_address = ctx.get::<SocketAddress>().cloned();
        let std_socket_addr = ctx.get::<SocketAddr>().copied();

        Self {
            rama_context: Some(ctx),
            httpward_context,
            secure_transport,
            socket_address,
            std_socket_addr,
            shared: HashMap::new(),
        }
    }

    /// Get immutable access to the captured HttpWard context.
    pub fn get_httpward_context(&self) -> Option<&HttpWardContext> {
        self.httpward_context.as_ref()
    }

    /// Get mutable access to the captured HttpWard context.
    ///
    /// Call `sync_to_rama_context()` to persist any changes back to the wrapped
    /// Rama context if the inner service needs to observe them.
    pub fn get_httpward_context_mut(&mut self) -> Option<&mut HttpWardContext> {
        self.httpward_context.as_mut()
    }

    /// Replace the captured HttpWard context explicitly.
    pub fn set_httpward_context(&mut self, value: HttpWardContext) {
        self.httpward_context = Some(value);
    }

    /// Get secure transport snapshot captured at request entry.
    pub fn get_secure_transport(&self) -> Option<&SecureTransport> {
        self.secure_transport.as_ref()
    }

    /// Get socket address snapshot from Rama context.
    pub fn get_socket_address(&self) -> Option<&SocketAddress> {
        self.socket_address.as_ref()
    }

    /// Get std::net::SocketAddr snapshot from Rama context.
    pub fn get_std_socket_addr(&self) -> Option<&SocketAddr> {
        self.std_socket_addr.as_ref()
    }

    /// Raw access to the wrapped Rama context for advanced middleware needs.
    pub fn rama_context(&self) -> Option<&Context<()>> {
        self.rama_context.as_ref()
    }

    /// Mutable raw access to wrapped Rama context for advanced middleware needs.
    pub fn rama_context_mut(&mut self) -> Option<&mut Context<()>> {
        self.rama_context.as_mut()
    }

    /// Insert cross-middleware shared JSON value.
    pub fn insert_shared_json(&mut self, key: impl Into<String>, value: Value) -> Option<Value> {
        self.shared.insert(key.into(), value)
    }

    /// Read cross-middleware shared JSON value.
    pub fn get_shared_json(&self, key: &str) -> Option<&Value> {
        self.shared.get(key)
    }

    /// Remove cross-middleware shared JSON value.
    pub fn remove_shared_json(&mut self, key: &str) -> Option<Value> {
        self.shared.remove(key)
    }

    /// Insert a typed serializable value into shared storage.
    pub fn insert_shared<T: Serialize>(
        &mut self,
        key: impl Into<String>,
        value: &T,
    ) -> Result<Option<Value>, serde_json::Error> {
        let json_value = serde_json::to_value(value)?;
        Ok(self.shared.insert(key.into(), json_value))
    }

    /// Read and deserialize a typed value from shared storage.
    pub fn get_shared_typed<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.shared
            .get(key)
            .and_then(|value| serde_json::from_value::<T>(value.clone()).ok())
    }

    /// Persist captured snapshots back to wrapped Rama context.
    pub fn sync_to_rama_context(&mut self) {
        if let Some(ctx) = self.rama_context.as_mut() {
            if let Some(httpward_ctx) = self.httpward_context.clone() {
                ctx.insert(httpward_ctx);
            }

            if let Some(secure_transport) = self.secure_transport.clone() {
                ctx.insert(secure_transport);
            }

            if let Some(socket_address) = self.socket_address {
                ctx.insert(socket_address);
            }

            if let Some(std_socket_addr) = self.std_socket_addr {
                ctx.insert(std_socket_addr);
            }
        }
    }

    /// Consume middleware context and return wrapped Rama context.
    ///
    /// Panics only if called after the context has already been consumed.
    pub fn take_rama_context(mut self) -> Context<()> {
        self.sync_to_rama_context();
        self.rama_context
            .take()
            .expect("HttpwardMiddlewareContext already consumed")
    }

    /// Extract wrapped Rama context from a mutable chain context.
    pub fn take_rama_context_from_chain(&mut self) -> Context<()> {
        self.sync_to_rama_context();
        self.rama_context
            .take()
            .expect("HttpwardMiddlewareContext already consumed")
    }
}
