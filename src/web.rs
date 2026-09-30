use crate::{
    accounting::{Price, Tokens},
    engine::{Engine, Fault, Result},
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::Utc;
use serde_json::{json, Value};

pub const PREFIX: &str = "/v0/management/plugins/cpa-apikey-manager";
pub const RESOURCE: &str = "/v0/resource/plugins/cpa-apikey-manager";

pub fn encode(text: &str) -> String {
    STANDARD.encode(text.as_bytes())
}
pub fn rejection(status: u16, body: &Value) -> Value {
    json!({"Terminate":true,"StatusCode":status,
        "ResponseHeaders":{"Content-Type":["application/json; charset=utf-8"],"Cache-Control":["no-store"]},
        "ResponseBody":encode(&body.to_string())})
}
pub fn response(status: u16, content_type: &str, body: &str) -> Value {
    json!({"StatusCode":status,"Headers":{
        "Content-Type":[content_type],"Cache-Control":["no-store"],
        "X-Content-Type-Options":["nosniff"],"Referrer-Policy":["no-referrer"],
        "Content-Security-Policy":["default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'"]
    },"Body":encode(body)})
}
pub fn registration(req: &Value) -> Value {
    let base = req["BasePath"].as_str().unwrap_or("/v0/management");
    let routes: Vec<_> = [
        ("GET","state"),("GET","reviews"),("PUT","policy"),("PUT","price"),
        ("POST","sync-models"),("POST","sync-prices"),("POST","model"),("POST","settle")
    ].iter().map(|(method,path)|json!({"Method":method,"Path":format!("{base}/plugins/cpa-apikey-manager/{path}")})).collect();
    json!({"routes":routes,"resources":[
        {"Path":"/index.html","Menu":"密钥权限与额度","Description":"按 API 密钥管理模型权限、Token 与金额额度"},
        {"Path":"/app.css"},{"Path":"/app.js"}
    ]})
}
pub fn resource(path: &str) -> Option<Value> {
    match path.strip_prefix(RESOURCE)? {
        "/index.html" => Some(response(
            200,
            "text/html; charset=utf-8",
            include_str!("../ui/index.html"),
        )),
        "/app.css" => Some(response(
            200,
            "text/css; charset=utf-8",
            include_str!("../ui/app.css"),
        )),
        "/app.js" => Some(response(
            200,
            "text/javascript; charset=utf-8",
            include_str!("../ui/app.js"),
        )),
        _ => None,
    }
}
fn query<'a>(req: &'a Value, name: &str) -> &'a str {
    req["Query"][name]
        .as_array()
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .unwrap_or("")
}
pub fn handle(engine: &mut Engine, req: &Value) -> Value {
    match action(engine, req) {
        Ok(value) => response(200, "application/json; charset=utf-8", &value.to_string()),
        Err(error) => response(
            error.status,
            "application/json; charset=utf-8",
            &error.json().to_string(),
        ),
    }
}
fn action(engine: &mut Engine, req: &Value) -> Result<Value> {
    let method = req["Method"].as_str().unwrap_or("");
    let route = req["Path"]
        .as_str()
        .unwrap_or("")
        .strip_prefix(PREFIX)
        .unwrap_or("");
    let body = match req["Body"].as_str() {
        Some(body) if body.len() <= 1_400_000 => {
            STANDARD.decode(body).map_err(|_| "请求编码无效")?
        }
        Some(_) => return Err(Fault::new(413, "body_too_large", "请求超过 1 MB 限制")),
        None => Vec::new(),
    };
    let data: Value = if body.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(&body)?
    };
    let required = |name: &str| -> Result<&str> {
        data[name]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| Fault::new(400, "missing_field", format!("缺少 {name}")))
    };
    match (method, route) {
        ("GET", "/state") => engine.snapshot(Utc::now()),
        ("GET", "/reviews") => {
            engine.ledger(query(req, "key"), query(req, "model"), 0, true, Utc::now())
        }
        ("PUT", "/policy") => engine.save_policy(
            required("key_id")?,
            serde_json::from_value(data["policy"].clone())?,
            data["reprice"].as_bool().unwrap_or(false),
            Utc::now(),
        ),
        ("PUT", "/price") => engine.save_price(
            required("model")?,
            serde_json::from_value::<Price>(data["price"].clone())?,
            data["revision"].as_i64().ok_or("缺少价格版本")?,
        ),
        ("POST", "/sync-models") => Ok(json!({"count":engine.sync_models(required("key_id")?)?})),
        ("POST", "/sync-prices") => engine.request_price_sync(),
        ("POST", "/model") => {
            let model = required("model")?;
            crate::engine::validate_model(model)?;
            engine
                .db
                .execute("INSERT OR IGNORE INTO models(id) VALUES(?1)", [model])?;
            engine.schedule_price_sync();
            Ok(json!({"model":model}))
        }
        ("POST", "/settle") => {
            engine.settle(
                required("request_id")?,
                required("model")?,
                serde_json::from_value::<Tokens>(data["tokens"].clone())?,
                required("reason")?,
                Utc::now(),
            )?;
            Ok(json!({"ok":true}))
        }
        _ => Err(Fault::new(404, "not_found", "接口不存在")),
    }
}
