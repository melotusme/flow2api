use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::sync::OnceLock;

pub fn catalog() -> &'static Value {
    static CATALOG: OnceLock<Value> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/models.json"))
            .expect("checked-in model catalog")
    })
}
pub fn model(id: &str) -> Result<Value> {
    catalog()
        .get(id)
        .cloned()
        .context("未知模型，请刷新模型清单")
}
pub fn public_models() -> Value {
    json!({"object":"list","data":catalog().as_object().unwrap().iter().filter(|(_,v)| v["config"]["listed"] != false).map(|(id,v)| json!({"id":id,"object":"model","owned_by":"flow2api","capabilities":v["capabilities"]})).collect::<Vec<_>>()})
}
