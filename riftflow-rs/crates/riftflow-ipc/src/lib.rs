//! Port of `src/main/shards/ipc/`.
//!
//! The wire contract is preserved exactly, because the Vue renderer is not
//! modified by this port:
//!
//! * `riftflowCall(namespace, fnName, ...args)` replies with
//!   `{ success: true, data }` or `{ success: false, error, isAxiosError? }`
//!   (`_standardizeIpcData`, invocation-router.ts:109).
//! * An unknown method **throws** instead of returning an envelope
//!   (invocation-router.ts:83), so it surfaces as a transport rejection.
//! * Renderer events go out as `riftflow-event` with positional args
//!   `[namespace, eventName, ...args]` and kebab-case event names.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const RIFTFLOW_IPC_MAIN_NAMESPACE: &str = "riftflow-ipc-main";
pub const CALL_CHANNEL: &str = "riftflowCall";
pub const REGISTER_CHANNEL: &str = "riftflowRendererRegister";
pub const EVENT_CHANNEL: &str = "riftflow-event";

pub type RendererId = u64;
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Delivers `riftflow-event` payloads to one renderer. Tauri's `emit_to` and
/// Electron's `webContents.send` both implement this.
pub trait RendererTransport: Send + Sync {
    fn send_to(&self, renderer: RendererId, event: &str, args: Vec<Value>) -> anyhow::Result<()>;
}

/// The `{ success, data }` / `{ success, error }` envelope.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IpcEnvelope {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(
        rename = "isAxiosError",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub is_axios_error: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
}

impl IpcEnvelope {
    /// `{ success: true; data: T }`
    pub fn ok(data: Value) -> Self {
        Self {
            success: true,
            data: Some(data),
            is_axios_error: None,
            error: None,
        }
    }

    /// `{ success: false; error: any }`
    pub fn failure(error: Value) -> Self {
        Self {
            success: false,
            data: None,
            is_axios_error: None,
            error: Some(error),
        }
    }

    /// `{ success: true }` with no `data` key, as `_processRendererRegister`
    /// returns it.
    pub fn success_bare() -> Self {
        Self {
            success: true,
            data: None,
            is_axios_error: None,
            error: None,
        }
    }

    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).expect("envelope is serializable")
    }
}

/// Errors a handler can raise; `to_envelope` reproduces
/// `toIpcErrorResponse` (error-response-factory.ts:10).
#[derive(Debug, Clone)]
pub enum IpcError {
    /// `RiftFlowIpcError`, a business error carrying a code
    Business { message: String, code: String },
    /// an axios-shaped HTTP failure
    Http {
        message: String,
        name: String,
        code: Option<String>,
        status: Option<i64>,
        status_text: Option<String>,
        body: Option<Value>,
    },
    /// any other throw
    Plain { message: String, name: String },
}

impl IpcError {
    pub fn business(message: impl Into<String>, code: impl Into<String>) -> Self {
        Self::Business { message: message.into(), code: code.into() }
    }

    /// Accepts anything displayable, since callers forward `anyhow::Error` as well
    /// as plain strings.
    pub fn plain(message: impl std::fmt::Display) -> Self {
        Self::Plain { message: message.to_string(), name: "Error".to_string() }
    }

    pub fn http(message: impl Into<String>, status: Option<i64>, body: Option<Value>) -> Self {
        Self::Http {
            message: message.into(),
            name: "AxiosError".to_string(),
            code: None,
            status,
            status_text: None,
            body,
        }
    }

    pub fn to_envelope(&self) -> IpcEnvelope {
        match self {
            IpcError::Http { message, name, code, status, status_text, body } => IpcEnvelope {
                success: false,
                data: None,
                is_axios_error: Some(true),
                error: Some(json!({
                    "response": if status.is_some() || body.is_some() {
                        json!({
                            "status": status,
                            "statusText": status_text,
                            "data": body.clone().unwrap_or(Value::Null),
                        })
                    } else {
                        Value::Null
                    },
                    "code": code,
                    "message": message,
                    "name": name,
                })),
            },
            IpcError::Business { message, code } => IpcEnvelope::failure(json!({
                "message": message,
                "name": "RiftFlowIpcError",
                "code": code,
            })),
            IpcError::Plain { message, name } => IpcEnvelope::failure(json!({
                "message": message,
                "name": name,
            })),
        }
    }
}

pub type IpcResult<T> = Result<T, IpcError>;

/// `event.sender.id` from the JS handler signature.
#[derive(Clone, Debug)]
pub struct CallContext {
    pub renderer: RendererId,
}

pub type CallHandler =
    Arc<dyn Fn(CallContext, Vec<Value>) -> BoxFuture<'static, IpcResult<Value>> + Send + Sync>;

/// What the transport should do with a completed call.
#[derive(Debug, PartialEq)]
pub enum CallOutcome {
    /// reply through the envelope
    Reply(IpcEnvelope),
    /// reject the invoke, like Electron rethrowing for an unknown function
    Rejected(String),
}

fn call_key(namespace: &str, fn_name: &str) -> String {
    format!("{namespace}:{fn_name}")
}

/// `RiftFlowIpcMain` plus its `RiftFlowIpcInvocationRouter`.
pub struct RiftFlowIpcMain {
    calls: RwLock<Vec<(String, CallHandler)>>,
    renderers: RwLock<Vec<RendererId>>,
    transport: Arc<dyn RendererTransport>,
}

impl RiftFlowIpcMain {
    pub fn new(transport: Arc<dyn RendererTransport>) -> Self {
        Self {
            calls: RwLock::new(Vec::new()),
            renderers: RwLock::new(Vec::new()),
            transport,
        }
    }

    /// `onCall(namespace, fnName, cb)` — duplicate keys are a hard error.
    pub fn on_call(&self, namespace: &str, fn_name: &str, handler: CallHandler) -> anyhow::Result<()> {
        let key = call_key(namespace, fn_name);
        let mut calls = self.calls.write().unwrap();

        if calls.iter().any(|(existing, _)| *existing == key) {
            anyhow::bail!(
                "Function \"{}\" in namespace \"{}\" already exists",
                fn_name,
                namespace
            );
        }

        calls.push((key, handler));
        Ok(())
    }

    /// The registered `(namespace, fnName)` pairs, sorted.
    ///
    /// The porting tooling compares this against the registrations in the Electron
    /// source; a name-level grep cannot see a handler registered under the wrong
    /// namespace, which is the failure that makes a renderer button dead.
    pub fn registered_calls(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = self
            .calls
            .read()
            .unwrap()
            .iter()
            .map(|(key, _)| match key.rsplit_once(':') {
                Some((namespace, fn_name)) => (namespace.to_string(), fn_name.to_string()),
                None => (key.clone(), String::new()),
            })
            .collect();
        out.sort();
        out
    }

    /// `ipcMain.handle('riftflowCall', ...)`.
    pub async fn handle_call(
        &self,
        renderer: RendererId,
        namespace: &str,
        fn_name: &str,
        args: Vec<Value>,
    ) -> CallOutcome {
        let key = call_key(namespace, fn_name);
        let found = self
            .calls
            .read()
            .unwrap()
            .iter()
            .find(|(existing, _)| *existing == key)
            .map(|(_, handler)| handler.clone());

        let Some(handler) = found else {
            return CallOutcome::Rejected(format!(
                "No function \"{}\" in namespace \"{}\"",
                fn_name, namespace
            ));
        };

        let future = handler(CallContext { renderer }, args);
        match future.await {
            Ok(value) => CallOutcome::Reply(IpcEnvelope::ok(value)),
            Err(error) => CallOutcome::Reply(error.to_envelope()),
        }
    }

    /// `ipcMain.handle('riftflowRendererRegister', ...)`. Note the JS original
    /// falls through to the error branch when re-registering a known renderer
    /// or unregistering an unknown one, so the quirk is kept.
    pub fn renderer_register(&self, renderer: RendererId, action: Option<&str>) -> IpcEnvelope {
        let action = action.unwrap_or("register");
        let mut renderers = self.renderers.write().unwrap();

        if action == "register" && !renderers.contains(&renderer) {
            renderers.push(renderer);
            return IpcEnvelope::success_bare();
        }

        if action == "unregister" && renderers.contains(&renderer) {
            renderers.retain(|existing| *existing != renderer);
            return IpcEnvelope::success_bare();
        }

        IpcEnvelope::failure(json!({ "message": format!("invalid action \"{action}\"") }))
    }

    /// Call this when a renderer window is destroyed.
    pub fn forget_renderer(&self, renderer: RendererId) {
        self.renderers.write().unwrap().retain(|existing| *existing != renderer);
    }

    pub fn registered_renderer_ids(&self) -> Vec<RendererId> {
        self.renderers.read().unwrap().clone()
    }

    /// `sendEvent(namespace, eventName, ...args)` to every registered renderer.
    pub fn send_event(&self, namespace: &str, event_name: &str, args: &[Value]) {
        let renderers = self.registered_renderer_ids();
        for renderer in renderers {
            self.send_event_to(renderer, namespace, event_name, args);
        }
    }

    /// `sendEventToWebContents(id, namespace, eventName, ...args)`
    pub fn send_event_to(&self, renderer: RendererId, namespace: &str, event_name: &str, args: &[Value]) {
        let mut payload = Vec::with_capacity(args.len() + 2);
        payload.push(Value::String(namespace.to_string()));
        payload.push(Value::String(event_name.to_string()));
        payload.extend_from_slice(args);

        if let Err(error) = self.transport.send_to(renderer, EVENT_CHANNEL, payload) {
            eprintln!("failed to emit riftflow-event to {renderer}: {error}");
        }
    }

    pub fn call_count(&self) -> usize {
        self.calls.read().unwrap().len()
    }
}

/// Helper for writing a handler as an `async move` block.
pub fn handler<F, Fut>(f: F) -> CallHandler
where
    F: Fn(CallContext, Vec<Value>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = IpcResult<Value>> + Send + 'static,
{
    Arc::new(move |ctx, args| Box::pin(f(ctx, args)))
}
