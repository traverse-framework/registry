//! Run a capability WASM fixture with optional `traverse_host::connector_invoke` mocking
//! under stock Traverse host resource limits (registry#635).
//!
//! Usage:
//!   wasm32-fixture-runner --wasm <path> [--connector-fixture <json-path>]
//!                         [--fuel N] [--max-memory-bytes N]
//!
//! Reads guest stdin from this process's stdin; writes guest stdout to this
//! process's stdout. Exit code mirrors the guest `_start` status (0 on success).
//! Exit code 3 means the artifact exceeds the configured host limits
//! (`capability.exceeds_host_limits` on stderr).
//!
//! Connector fixture JSON shape (registry#576):
//! ```json
//! {
//!   "activated": true,
//!   "responses": [
//!     {
//!       "connector_id": "traverse.object-store",
//!       "operation": "put_immutable",
//!       "body": {"abi_version":"1.0.0","result_class":"ok","payload":{}}
//!     }
//!   ]
//! }
//! ```
//! When `activated` is false, every `connector_invoke` returns the Spec 104
//! unbound code (`-2`) without reading guest memory.

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;
use wasmtime::{
    Caller, Config, Engine, Extern, Linker, Module, Store, StoreLimits, StoreLimitsBuilder,
};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime_wasi::WasiCtxBuilder;

/// Spec 104 / Traverse executor: no activated binding.
const CONNECTOR_INVOKE_ERR_UNBOUND: i32 = -2;
const CONNECTOR_INVOKE_ERR_INVALID_REQUEST: i32 = -1;
const CONNECTOR_INVOKE_ERR_EXECUTION_FAILED: i32 = -6;
const MAX_CONNECTOR_INVOKE_REQUEST_BYTES: usize = 64 * 1024;
const MAX_CONNECTOR_INVOKE_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_STDOUT_BYTES: usize = 256 * 1024;

/// Stock Traverse native `WasmExecutor` defaults (traverse-runtime 0.15.0).
/// Memory matches `runtime.wasm` nested default (32 MiB). Fuel uses the
/// stricter native default (5_000_000 vs nested 10_000_000) so an artifact
/// that passes this gate fits both stock product hosts.
const PRODUCT_FUEL: u64 = 5_000_000;
const PRODUCT_MEMORY_BYTES: usize = 32 * 1024 * 1024;
const PRODUCT_TABLE_ELEMENTS: usize = 1_024;
const PRODUCT_INSTANCES: usize = 1;
const PRODUCT_TABLES: usize = 8;
const PRODUCT_MEMORIES: usize = 1;

/// Machine-readable pin recorded in CI output (registry#635 AC).
const HOST_LIMITS_PIN: &str =
    "traverse-runtime WasmExecutor defaults@0.15.0 fuel=5000000 memory_bytes=33554432";

/// Process exit code when the guest exceeds host fuel/memory limits.
const EXIT_EXCEEDS_HOST_LIMITS: u8 = 3;

#[derive(Debug, Deserialize, Clone)]
struct ConnectorFixture {
    #[serde(default = "default_activated")]
    activated: bool,
    #[serde(default)]
    responses: Vec<ConnectorResponseEntry>,
}

fn default_activated() -> bool {
    true
}

#[derive(Debug, Deserialize, Clone)]
struct ConnectorResponseEntry {
    connector_id: String,
    operation: String,
    body: Value,
}

struct HostState {
    wasi: WasiP1Ctx,
    fixture: Option<Arc<ConnectorFixture>>,
    limits: StoreLimits,
}

#[derive(Debug, Clone, Copy)]
struct HostLimits {
    fuel: u64,
    memory_bytes: usize,
}

impl Default for HostLimits {
    fn default() -> Self {
        Self {
            fuel: PRODUCT_FUEL,
            memory_bytes: PRODUCT_MEMORY_BYTES,
        }
    }
}

fn main() -> ExitCode {
    match run_cli() {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            if message.starts_with("capability.exceeds_host_limits") {
                eprintln!("wasm32-fixture-runner: {message}");
                return ExitCode::from(EXIT_EXCEEDS_HOST_LIMITS);
            }
            eprintln!("wasm32-fixture-runner: {message}");
            ExitCode::from(2)
        }
    }
}

fn run_cli() -> Result<u8, String> {
    let args = parse_args(env::args().skip(1))?;
    eprintln!("host_limits_pin: {HOST_LIMITS_PIN}");
    eprintln!(
        "host_limits_effective: fuel={} memory_bytes={}",
        args.limits.fuel, args.limits.memory_bytes
    );

    let wasm_bytes = fs::read(&args.wasm)
        .map_err(|err| format!("unable to read wasm '{}': {err}", args.wasm.display()))?;

    let fixture = match args.connector_fixture {
        Some(path) => {
            let text = fs::read_to_string(&path).map_err(|err| {
                format!(
                    "unable to read connector fixture '{}': {err}",
                    path.display()
                )
            })?;
            let parsed: ConnectorFixture = serde_json::from_str(&text).map_err(|err| {
                format!("invalid connector fixture JSON '{}': {err}", path.display())
            })?;
            Some(parsed)
        }
        None => None,
    };

    let mut stdin_bytes = Vec::new();
    io::stdin()
        .read_to_end(&mut stdin_bytes)
        .map_err(|err| format!("unable to read stdin: {err}"))?;

    let (code, stdout) =
        execute_wasm_fixture(&wasm_bytes, &stdin_bytes, fixture.as_ref(), args.limits)?;
    io::stdout()
        .write_all(&stdout)
        .map_err(|err| format!("unable to write stdout: {err}"))?;
    Ok(code)
}

fn execute_wasm_fixture(
    wasm_bytes: &[u8],
    stdin_bytes: &[u8],
    fixture: Option<&ConnectorFixture>,
    limits: HostLimits,
) -> Result<(u8, Vec<u8>), String> {
    let fixture = fixture.cloned().map(Arc::new);

    let stdout_pipe = MemoryOutputPipe::new(MAX_STDOUT_BYTES);
    let stdout_reader = stdout_pipe.clone();
    let wasi_ctx: WasiP1Ctx = WasiCtxBuilder::new()
        .stdin(MemoryInputPipe::new(stdin_bytes.to_vec()))
        .stdout(stdout_pipe)
        .inherit_stderr()
        .build_p1();

    let mut config = Config::new();
    config.consume_fuel(true);
    let engine = Engine::new(&config).map_err(|err| format!("unable to create engine: {err}"))?;
    let module = Module::new(&engine, wasm_bytes)
        .map_err(|err| format!("unable to compile wasm module: {err}"))?;

    if let Some(required) = module_initial_memory_bytes(&module) {
        if required > limits.memory_bytes {
            return Err(format!(
                "capability.exceeds_host_limits: required_memory_bytes={required} \
                 allowed_memory_bytes={} required_fuel=unknown allowed_fuel={} \
                 (initial linear memory exceeds stock Traverse WasmExecutor ceiling)",
                limits.memory_bytes, limits.fuel
            ));
        }
    }

    let mut linker: Linker<HostState> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |state: &mut HostState| &mut state.wasi)
        .map_err(|err| format!("unable to link WASI: {err}"))?;

    if fixture.is_some() {
        linker
            .func_wrap("traverse_host", "connector_invoke", handle_connector_invoke)
            .map_err(|err| format!("unable to register connector_invoke: {err}"))?;
    }

    if fixture.is_none() {
        for import in module.imports() {
            if import.module() == "traverse_host" && import.name() == "connector_invoke" {
                return Err(
                    "guest imports traverse_host::connector_invoke but no --connector-fixture was provided"
                        .to_string(),
                );
            }
        }
    }

    let store_limits = StoreLimitsBuilder::new()
        .memory_size(limits.memory_bytes)
        .table_elements(PRODUCT_TABLE_ELEMENTS)
        .instances(PRODUCT_INSTANCES)
        .tables(PRODUCT_TABLES)
        .memories(PRODUCT_MEMORIES)
        .trap_on_grow_failure(true)
        .build();

    let mut store = Store::new(
        &engine,
        HostState {
            wasi: wasi_ctx,
            fixture,
            limits: store_limits,
        },
    );
    store.limiter(|state| &mut state.limits);
    store
        .set_fuel(limits.fuel)
        .map_err(|err| format!("unable to set fuel: {err}"))?;

    let instance = linker.instantiate(&mut store, &module).map_err(|err| {
        classify_resource_error(
            &err.to_string(),
            limits,
            "unable to instantiate wasm module",
        )
    })?;
    let start = instance
        .get_typed_func::<(), ()>(&mut store, "_start")
        .map_err(|err| format!("wasm module missing _start export: {err}"))?;

    let code = match start.call(&mut store, ()) {
        Ok(()) => 0_u8,
        Err(err) => {
            // WASI preview1 implements `proc_exit` by returning `I32Exit`.
            // Zero is successful completion (stdout still holds the result).
            match err.downcast_ref::<wasmtime_wasi::I32Exit>() {
                Some(exit) if exit.0 == 0 => 0,
                Some(exit) => {
                    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                    {
                        exit.0.clamp(1, 255) as u8
                    }
                }
                None => {
                    let message = err.to_string();
                    if looks_like_resource_limit(&message) {
                        return Err(classify_resource_error(
                            &message,
                            limits,
                            "guest trapped on resource limit",
                        ));
                    }
                    eprintln!("wasm32-fixture-runner: guest trap: {err}");
                    1
                }
            }
        }
    };

    Ok((code, stdout_reader.contents().to_vec()))
}

fn module_initial_memory_bytes(module: &Module) -> Option<usize> {
    // Prefer an exported memory; fall back to the first memory import/export type.
    for export in module.exports() {
        if let wasmtime::ExternType::Memory(ty) = export.ty() {
            let pages = usize::try_from(ty.minimum()).ok()?;
            return pages.checked_mul(64 * 1024);
        }
    }
    for import in module.imports() {
        if let wasmtime::ExternType::Memory(ty) = import.ty() {
            let pages = usize::try_from(ty.minimum()).ok()?;
            return pages.checked_mul(64 * 1024);
        }
    }
    None
}

fn looks_like_resource_limit(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("all fuel consumed")
        || (lower.contains("fuel")
            && (lower.contains("exhausted") || lower.contains("consumed")))
        || lower.contains("resource limiter")
        || (lower.contains("memory")
            && (lower.contains("grow") || lower.contains("maximum") || lower.contains("limit")))
}

fn classify_resource_error(message: &str, limits: HostLimits, context: &str) -> String {
    let lower = message.to_ascii_lowercase();
    let fuel_hit = lower.contains("fuel");
    let memory_hit = lower.contains("memory") || lower.contains("resource limiter");
    let required_memory = if memory_hit {
        format!(">{}", limits.memory_bytes)
    } else {
        "unknown".to_string()
    };
    let required_fuel = if fuel_hit {
        format!(">{}", limits.fuel)
    } else {
        "unknown".to_string()
    };
    format!(
        "capability.exceeds_host_limits: required_memory_bytes={required_memory} \
         allowed_memory_bytes={} required_fuel={required_fuel} allowed_fuel={} \
         ({context}: {message})",
        limits.memory_bytes, limits.fuel
    )
}

fn handle_connector_invoke(
    mut caller: Caller<'_, HostState>,
    request_ptr: i32,
    request_len: i32,
    response_ptr: i32,
    response_capacity: i32,
) -> i32 {
    let Some(fixture) = caller.data().fixture.clone() else {
        return CONNECTOR_INVOKE_ERR_UNBOUND;
    };
    if !fixture.activated {
        // Fail closed before reading guest memory — mirrors Traverse's unbound
        // default handler (no host-private data crosses the boundary).
        return CONNECTOR_INVOKE_ERR_UNBOUND;
    }

    if request_ptr < 0 || request_len < 0 || response_ptr < 0 || response_capacity < 0 {
        return CONNECTOR_INVOKE_ERR_INVALID_REQUEST;
    }
    let request_len = request_len as usize;
    let response_ptr = response_ptr as usize;
    let response_capacity = response_capacity as usize;
    let request_ptr = request_ptr as usize;
    if request_len > MAX_CONNECTOR_INVOKE_REQUEST_BYTES
        || response_capacity > MAX_CONNECTOR_INVOKE_RESPONSE_BYTES
    {
        return CONNECTOR_INVOKE_ERR_INVALID_REQUEST;
    }

    let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
        return CONNECTOR_INVOKE_ERR_INVALID_REQUEST;
    };

    let mut request_bytes = vec![0_u8; request_len];
    if memory
        .read(&caller, request_ptr, &mut request_bytes)
        .is_err()
    {
        return CONNECTOR_INVOKE_ERR_INVALID_REQUEST;
    }

    let Ok(request) = serde_json::from_slice::<ConnectorInvokeRequest>(&request_bytes) else {
        return CONNECTOR_INVOKE_ERR_INVALID_REQUEST;
    };
    if request.abi_version.is_empty()
        || request.connector_id.is_empty()
        || request.operation.is_empty()
    {
        return CONNECTOR_INVOKE_ERR_INVALID_REQUEST;
    }

    // Empty fixture operation is a wildcard — publishers often only know the
    // connector_id from connector_requirements, not every guest operation name.
    let Some(entry) = fixture.responses.iter().find(|entry| {
        entry.connector_id == request.connector_id
            && (entry.operation.is_empty() || entry.operation == request.operation)
    }) else {
        return CONNECTOR_INVOKE_ERR_EXECUTION_FAILED;
    };

    let Ok(response_bytes) = serde_json::to_vec(&entry.body) else {
        return CONNECTOR_INVOKE_ERR_EXECUTION_FAILED;
    };
    if response_bytes.len() > response_capacity {
        return CONNECTOR_INVOKE_ERR_EXECUTION_FAILED;
    }
    if memory
        .write(&mut caller, response_ptr, &response_bytes)
        .is_err()
    {
        return CONNECTOR_INVOKE_ERR_EXECUTION_FAILED;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    {
        response_bytes.len() as i32
    }
}

#[derive(Debug, Deserialize)]
struct ConnectorInvokeRequest {
    abi_version: String,
    connector_id: String,
    operation: String,
    #[serde(default)]
    #[allow(dead_code)]
    payload: Value,
}

struct Args {
    wasm: PathBuf,
    connector_fixture: Option<PathBuf>,
    limits: HostLimits,
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut wasm = None;
    let mut connector_fixture = None;
    let mut limits = HostLimits::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--wasm" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--wasm requires a path argument".to_string())?;
                wasm = Some(PathBuf::from(value));
            }
            "--connector-fixture" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--connector-fixture requires a path argument".to_string())?;
                connector_fixture = Some(PathBuf::from(value));
            }
            "--fuel" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--fuel requires an integer argument".to_string())?;
                limits.fuel = value
                    .parse::<u64>()
                    .map_err(|_| format!("invalid --fuel value '{value}'"))?;
                if limits.fuel == 0 {
                    return Err("--fuel must be greater than zero".to_string());
                }
            }
            "--max-memory-bytes" => {
                let value = args.next().ok_or_else(|| {
                    "--max-memory-bytes requires an integer argument".to_string()
                })?;
                limits.memory_bytes = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --max-memory-bytes value '{value}'"))?;
                if limits.memory_bytes == 0 {
                    return Err("--max-memory-bytes must be greater than zero".to_string());
                }
            }
            "--help" | "-h" => {
                return Err(
                    "Usage: wasm32-fixture-runner --wasm <path> [--connector-fixture <json>] \
                     [--fuel N] [--max-memory-bytes N]"
                        .to_string(),
                );
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    let wasm = wasm.ok_or_else(|| "missing required --wasm <path>".to_string())?;
    Ok(Args {
        wasm,
        connector_fixture,
        limits,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unbound_fixture_is_inactive() {
        let fixture = ConnectorFixture {
            activated: false,
            responses: vec![],
        };
        assert!(!fixture.activated);
    }

    #[test]
    fn response_entry_deserializes() {
        let raw = r#"{
            "activated": true,
            "responses": [{
                "connector_id": "traverse.object-store",
                "operation": "put_immutable",
                "body": {"abi_version":"1.0.0","result_class":"ok","payload":{"asset_ref":"a"}}
            }]
        }"#;
        let fixture: ConnectorFixture = match serde_json::from_str(raw) {
            Ok(value) => value,
            Err(err) => panic!("fixture parses: {err}"),
        };
        assert!(fixture.activated);
        assert_eq!(fixture.responses.len(), 1);
        assert_eq!(fixture.responses[0].connector_id, "traverse.object-store");
    }

    #[test]
    fn wat_module_compiles() {
        let mut config = Config::new();
        config.consume_fuel(true);
        let engine = Engine::new(&config).expect("engine");
        let module = Module::new(
            &engine,
            r#"(module (memory (export "memory") 1) (func (export "_start")))"#,
        );
        assert!(module.is_ok(), "{module:?}");
    }

    /// Minimal guest that calls `connector_invoke` once and writes either the
    /// response body or the ASCII marker `UNBOUND` when the host returns < 0.
    fn connector_probe_wasm() -> Vec<u8> {
        let wat = r#"(module
          (import "wasi_snapshot_preview1" "fd_write"
            (func $fd_write (param i32 i32 i32 i32) (result i32)))
          (import "traverse_host" "connector_invoke"
            (func $connector_invoke (param i32 i32 i32 i32) (result i32)))
          (memory (export "memory") 1)
          (data (i32.const 0) "{\"abi_version\":\"1.0.0\",\"connector_id\":\"traverse.object-store\",\"operation\":\"put_immutable\",\"payload\":{}}")
          (data (i32.const 800) "UNBOUND")
          (func (export "_start")
            (local $n i32)
            (local.set $n
              (call $connector_invoke
                (i32.const 0) (i32.const 103)
                (i32.const 256) (i32.const 256)))
            (if (i32.lt_s (local.get $n) (i32.const 0))
              (then
                (i32.store (i32.const 900) (i32.const 800))
                (i32.store (i32.const 904) (i32.const 7))
                (drop (call $fd_write (i32.const 1) (i32.const 900) (i32.const 1) (i32.const 920))))
              (else
                (i32.store (i32.const 900) (i32.const 256))
                (i32.store (i32.const 904) (local.get $n))
                (drop (call $fd_write (i32.const 1) (i32.const 900) (i32.const 1) (i32.const 920)))))
          )
        )"#;
        match wat::parse_str(wat) {
            Ok(bytes) => bytes,
            Err(err) => panic!("probe wat encodes: {err}"),
        }
    }

    #[test]
    fn positive_activated_connector_returns_fixture_body() {
        let fixture = ConnectorFixture {
            activated: true,
            responses: vec![ConnectorResponseEntry {
                connector_id: "traverse.object-store".to_string(),
                operation: "put_immutable".to_string(),
                body: serde_json::json!({
                    "abi_version": "1.0.0",
                    "result_class": "ok",
                    "payload": {"asset_ref": "asset:1"}
                }),
            }],
        };
        let (code, stdout) = match execute_wasm_fixture(
            &connector_probe_wasm(),
            b"{}",
            Some(&fixture),
            HostLimits::default(),
        ) {
            Ok(result) => result,
            Err(err) => panic!("execute: {err}"),
        };
        assert_eq!(code, 0, "stdout={}", String::from_utf8_lossy(&stdout));
        let value: Value = match serde_json::from_slice(&stdout) {
            Ok(value) => value,
            Err(err) => panic!("json stdout: {err}"),
        };
        assert_eq!(value["payload"]["asset_ref"], "asset:1");
    }

    #[test]
    fn negative_missing_activation_fails_closed() {
        let fixture = ConnectorFixture {
            activated: false,
            responses: vec![],
        };
        let (code, stdout) = match execute_wasm_fixture(
            &connector_probe_wasm(),
            b"{}",
            Some(&fixture),
            HostLimits::default(),
        ) {
            Ok(result) => result,
            Err(err) => panic!("execute: {err}"),
        };
        assert_eq!(code, 0, "stdout={}", String::from_utf8_lossy(&stdout));
        assert_eq!(stdout, b"UNBOUND");
    }

    #[test]
    fn module_with_oversized_initial_memory_is_rejected() {
        // 600 pages * 64 KiB = 39_321_600 bytes > 32 MiB product ceiling.
        let wasm = wat::parse_str(
            r#"(module
              (memory (export "memory") 600)
              (func (export "_start"))
            )"#,
        )
        .expect("wat");
        let err = execute_wasm_fixture(&wasm, b"{}", None, HostLimits::default())
            .expect_err("oversized initial memory must fail");
        assert!(
            err.starts_with("capability.exceeds_host_limits"),
            "unexpected err: {err}"
        );
        assert!(err.contains("required_memory_bytes=39321600"), "{err}");
        assert!(err.contains("allowed_memory_bytes=33554432"), "{err}");
    }

    #[test]
    fn module_within_product_limits_passes() {
        let wasm = wat::parse_str(
            r#"(module
              (memory (export "memory") 1)
              (func (export "_start"))
            )"#,
        )
        .expect("wat");
        let (code, _stdout) =
            execute_wasm_fixture(&wasm, b"{}", None, HostLimits::default()).expect("within limits");
        assert_eq!(code, 0);
    }

    #[test]
    fn host_limits_pin_mentions_traverse_runtime() {
        assert!(HOST_LIMITS_PIN.contains("traverse-runtime"));
        assert!(HOST_LIMITS_PIN.contains("fuel=5000000"));
        assert!(HOST_LIMITS_PIN.contains("memory_bytes=33554432"));
    }
}
