//! specs/020-public-execution-proxy: a thin Cloudflare Worker.
//!
//! Holds the `traverse-cli serve` admin credential privately (a Worker
//! secret, never returned in any response), validates a request against
//! this registry's own published catalog before forwarding, rate-limits
//! per caller, and forwards only validated requests to `serve`'s
//! verified-entrypoint endpoint (traverse spec 115,
//! `POST {serve}/v1/entrypoints/execute`). `serve`'s own trust model,
//! specs, and code are untouched -- this Worker is just an ordinary
//! admin-scoped client of an already-governed, already-approved surface.
//!
//! FR numbers below refer to `specs/020-public-execution-proxy/spec.md`.

use serde::{Deserialize, Serialize};
use worker::*;

/// This registry's own published, CORS-enabled, always-fresh capability
/// catalog (rebuilt and redeployed by `deploy-catalog` on every merge to
/// `main`). FR-002 asks to validate against "this registry's own
/// published public index (`index.json`)" -- `index.json` itself is a
/// versioned GitHub Release asset with no stable "latest" URL that can be
/// trusted without an authenticated, rate-limited GitHub API call on
/// every proxied request (GitHub's unauthenticated API limit is shared
/// across all callers from Cloudflare's edge IP ranges). `catalog.json`
/// carries the same namespace/id/version/deprecated identity data, built
/// from the same source on the same merge trigger, and is free to fetch
/// with no rate-limit risk -- the deliberately chosen, more reliable
/// source for the same validation, documented here rather than silently
/// substituted.
const CATALOG_URL: &str = "https://registry.traverse-framework.com/catalog.json";

/// Cloudflare edge-cache TTL (seconds) for the catalog fetch. Short enough
/// that a just-deprecated capability stops validating within a minute;
/// long enough that this proxy doesn't hit the catalog on every single
/// request.
const CATALOG_CACHE_SECONDS: i32 = 60;

/// Default request-body size cap, overridable via the `MAX_REQUEST_BODY_BYTES`
/// var so an operator can tune it to stay at or below whatever the paired
/// `serve` instance's own `MAX_REQUEST_BODY` is configured to (FR-004).
/// 256 KiB is generous for a single capability's inline execution request.
const DEFAULT_MAX_REQUEST_BODY_BYTES: usize = 262_144;

#[derive(Debug, Deserialize)]
struct ExecuteRequestBody {
    entrypoint_kind: String,
    id: String,
    version: String,
    request: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct ProblemBody {
    traverse_code: String,
    detail: String,
}

#[derive(Debug, Deserialize)]
struct CatalogContractIdentity {
    id: String,
    version: String,
}

#[derive(Debug, Deserialize)]
struct CatalogCapabilityEntry {
    deprecated: bool,
    contract: CatalogContractIdentity,
}

#[derive(Debug, Deserialize)]
struct Catalog {
    capabilities: Vec<CatalogCapabilityEntry>,
}

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    Router::new()
        .post_async("/execute", handle_execute)
        .options_async("/execute", handle_options)
        .run(req, env)
        .await
}

/// FR-002/FR-003/FR-004/FR-005/FR-006 in order: size cap, parse, scope
/// check, rate limit, catalog validation, then forward with the credential.
async fn handle_execute(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let env = &ctx.env;

    let max_bytes = env
        .var("MAX_REQUEST_BODY_BYTES")
        .ok()
        .and_then(|v| v.to_string().parse::<usize>().ok())
        .unwrap_or(DEFAULT_MAX_REQUEST_BODY_BYTES);

    let body_bytes = match req.bytes().await {
        Ok(bytes) => bytes,
        Err(_) => {
            return problem(
                env,
                400,
                "invalid_entrypoint_request",
                "unable to read request body",
            );
        }
    };
    if body_bytes.len() > max_bytes {
        return problem(
            env,
            413,
            "request_body_too_large",
            "request body exceeds the configured limit",
        );
    }

    let parsed: ExecuteRequestBody = match serde_json::from_slice(&body_bytes) {
        Ok(parsed) => parsed,
        Err(_) => {
            return problem(
                env,
                400,
                "invalid_entrypoint_request",
                "request body must be JSON with entrypoint_kind, id, version, and request",
            );
        }
    };

    // Spec 020's own scope is single-capability execution only (see
    // "Scope: single-capability execution only" in the spec) -- this
    // proxy deliberately does not support any other entrypoint_kind.
    if parsed.entrypoint_kind != "capability" {
        return problem(
            env,
            400,
            "unsupported_entrypoint_kind",
            "this proxy only supports entrypoint_kind \"capability\"",
        );
    }
    if parsed.id.trim().is_empty() || parsed.version.trim().is_empty() {
        return problem(
            env,
            400,
            "invalid_entrypoint_request",
            "id and version are both required and must be non-empty",
        );
    }

    // FR-003: rate limit before any further work -- in particular, before
    // the catalog fetch and before ever reaching serve.
    let client_ip = req
        .headers()
        .get("CF-Connecting-IP")
        .ok()
        .flatten()
        .unwrap_or_else(|| "unknown".to_string());
    let limiter = env.rate_limiter("EXECUTE_RATE_LIMITER")?;
    let outcome = limiter.limit(client_ip).await?;
    if !outcome.success {
        return problem(
            env,
            429,
            "rate_limited",
            "too many requests from this address; try again shortly",
        );
    }

    // FR-002: validate against the published catalog before forwarding.
    match validate_against_catalog(&parsed.id, &parsed.version).await {
        Ok(true) => {}
        Ok(false) => {
            return problem(
                env,
                404,
                "capability_not_found",
                "no non-deprecated published capability matches the requested id/version",
            );
        }
        Err(_) => {
            return problem(
                env,
                502,
                "catalog_unavailable",
                "unable to validate the request against the published catalog",
            );
        }
    }

    forward_to_serve(env, &parsed).await
}

async fn validate_against_catalog(id: &str, version: &str) -> Result<bool> {
    let mut init = RequestInit::new();
    init.cf = CfProperties {
        cache_ttl: Some(CATALOG_CACHE_SECONDS),
        cache_everything: Some(true),
        ..CfProperties::default()
    };
    let catalog_req = Request::new_with_init(CATALOG_URL, &init)?;
    let mut resp = Fetch::Request(catalog_req).send().await?;
    let catalog: Catalog = resp.json().await?;
    Ok(catalog.capabilities.iter().any(|entry| {
        !entry.deprecated && entry.contract.id == id && entry.contract.version == version
    }))
}

/// FR-005/FR-006: forwards unmodified in substance, with the admin
/// credential attached only here, server-side -- never echoed back in any
/// response, log line, or error message (this function never logs the
/// header or the credential var, and every error path above returns a
/// fixed, static detail string rather than anything derived from
/// upstream response bodies that could carry it).
async fn forward_to_serve(env: &Env, parsed: &ExecuteRequestBody) -> Result<Response> {
    let serve_url = match env.var("SERVE_URL") {
        Ok(v) => v.to_string(),
        Err(_) => {
            return problem(
                env,
                500,
                "proxy_misconfigured",
                "SERVE_URL is not configured",
            );
        }
    };
    let admin_jwt = match env.secret("ADMIN_JWT") {
        Ok(v) => v.to_string(),
        Err(_) => {
            return problem(
                env,
                500,
                "proxy_misconfigured",
                "ADMIN_JWT is not configured",
            );
        }
    };

    let forward_body = serde_json::json!({
        "entrypoint_kind": "capability",
        "id": parsed.id,
        "version": parsed.version,
        "request": parsed.request,
    });

    let headers = Headers::new();
    headers.set("Content-Type", "application/json")?;
    headers.set("Authorization", &format!("Bearer {admin_jwt}"))?;

    let mut init = RequestInit::new();
    init.method = Method::Post;
    init.headers = headers;
    init.body = Some(wasm_bindgen::JsValue::from_str(&forward_body.to_string()));

    let target = format!("{}/v1/entrypoints/execute", serve_url.trim_end_matches('/'));
    let upstream_req = Request::new_with_init(&target, &init)?;

    let mut upstream_resp = match Fetch::Request(upstream_req).send().await {
        Ok(resp) => resp,
        Err(_) => {
            // Deliberately a fixed message, not the underlying fetch
            // error -- a connection-level error could otherwise leak
            // internal networking details about the serve backend.
            return problem(
                env,
                502,
                "serve_unreachable",
                "unable to reach the execution backend",
            );
        }
    };

    let status = upstream_resp.status_code();
    let bytes = upstream_resp.bytes().await.unwrap_or_default();

    let mut final_resp = Response::from_bytes(bytes)?.with_status(status);
    final_resp
        .headers_mut()
        .set("Content-Type", "application/json")?;
    apply_cors(env, &mut final_resp)?;
    Ok(final_resp)
}

async fn handle_options(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let mut resp = Response::empty()?.with_status(204);
    apply_cors(&ctx.env, &mut resp)?;
    Ok(resp)
}

/// Restricts the browser-callable origin (per the brainstorm decision
/// recorded in `docs/decision-log.md` entry 73): an open
/// `Access-Control-Allow-Origin: *` would let any third-party page embed
/// a button that fires real WASM execution using a visiting browser as
/// the caller -- an amplification vector against the per-IP rate limit
/// itself (many different visiting browsers, each individually within
/// limit). FR-002/FR-003 still gate abuse independently; this closes a
/// distinct hole neither of them addresses.
fn apply_cors(env: &Env, resp: &mut Response) -> Result<()> {
    let allowed_origin = env
        .var("ALLOWED_ORIGIN")
        .map(|v| v.to_string())
        .unwrap_or_default();
    let headers = resp.headers_mut();
    headers.set("Access-Control-Allow-Origin", &allowed_origin)?;
    headers.set("Access-Control-Allow-Methods", "POST, OPTIONS")?;
    headers.set("Access-Control-Allow-Headers", "Content-Type")?;
    headers.set("Vary", "Origin")?;
    Ok(())
}

/// A stable `application/problem+json` error envelope, matching the
/// `traverse_code`/`detail` shape `serve` itself already uses (traverse
/// spec 033) so a caller never has to distinguish "the proxy rejected
/// this" from "serve rejected this" by response shape alone.
fn problem(env: &Env, status: u16, code: &str, detail: &str) -> Result<Response> {
    let body = ProblemBody {
        traverse_code: code.to_string(),
        detail: detail.to_string(),
    };
    let mut resp = Response::from_json(&body)?.with_status(status);
    resp.headers_mut()
        .set("Content-Type", "application/problem+json")?;
    apply_cors(env, &mut resp)?;
    Ok(resp)
}
