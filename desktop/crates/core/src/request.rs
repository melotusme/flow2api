//! Normalize client conversations using the archived Python request semantics.
use crate::{catalog, generation, model::*, resolver};
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

fn empty(model: &str) -> GenerationRequest {
    GenerationRequest {
        model: model.into(),
        prompt: String::new(),
        images: vec![],
        account_id: None,
        video_id: None,
    }
}

pub fn openai(body: &Value) -> Result<GenerationRequest> {
    let model = body["model"].as_str().context("缺少 model")?;
    let Some(message) = body["messages"]
        .as_array()
        .and_then(|messages| messages.last())
    else {
        if let Some(prompt) = body["prompt"].as_str() {
            let mut request = empty(model);
            request.prompt = prompt.trim().into();
            request.images = body["images"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
            if request.images.is_empty() {
                if let Some(image) = body["image"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    request.images.push(image.into());
                }
            }
            request.video_id = body["video_id"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            request.account_id = body["account_id"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            return Ok(request);
        }
        return gemini(model, body);
    };
    let mut request = empty(model);
    let mut texts = vec![];
    if let Some(text) = message["content"].as_str() {
        texts.push(text.trim());
    }
    for part in message["content"].as_array().into_iter().flatten() {
        match part["type"].as_str() {
            Some("text") => {
                if let Some(text) = part["text"].as_str() {
                    texts.push(text.trim());
                }
            }
            Some("image_url") => {
                let url = part["image_url"]["url"]
                    .as_str()
                    .context("缺少 image_url.url")?;
                if let Some(id) = url.strip_prefix("extend://") {
                    request.video_id = Some(id.into());
                } else {
                    request.images.push(url.into());
                }
            }
            _ => {}
        }
    }
    if request.images.is_empty() {
        if let Some(image) = body["image"].as_str().filter(|s| !s.is_empty()) {
            request.images.push(image.into());
        }
    }
    if request.video_id.is_none() {
        request.video_id = body["video_id"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
    }
    if request.account_id.is_none() {
        request.account_id = body["account_id"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
    }
    request.prompt = texts
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    Ok(request)
}

fn sanitize(prompt: &str) -> String {
    static TOOLS: OnceLock<Regex> = OnceLock::new();
    static PREAMBLE: OnceLock<Regex> = OnceLock::new();
    let tools = TOOLS.get_or_init(|| Regex::new(r"(?is)<tools>.*?</tools>").unwrap());
    let preamble = PREAMBLE.get_or_init(|| Regex::new(r"(?i)^(you are a function calling ai model\.?|you are provided with function signatures within .* xml tags\.?|you may call one or more functions to assist with the user query\.?|don't make assumptions about what values to plug into functions\.?|here are the available tools:.*)$").unwrap());
    let stripped = tools.replace_all(prompt.trim(), " ");
    let mut lines: Vec<&str> = vec![];
    for line in stripped.lines().map(str::trim) {
        if preamble.is_match(line) {
            continue;
        }
        if line.is_empty() && lines.last().is_none_or(|previous| previous.is_empty()) {
            continue;
        }
        lines.push(line);
    }
    lines.join("\n").trim().into()
}

pub fn gemini(model: &str, body: &Value) -> Result<GenerationRequest> {
    let contents = body["contents"]
        .as_array()
        .filter(|c| !c.is_empty())
        .context("contents 不能为空")?;
    let target = contents
        .iter()
        .rev()
        .find(|content| content["role"].as_str().unwrap_or("user") == "user")
        .unwrap_or_else(|| contents.last().unwrap());
    let mut request = empty(model);
    let mut texts = vec![];
    for part in target["parts"].as_array().into_iter().flatten() {
        if let Some(text) = part["text"].as_str().filter(|s| !s.is_empty()) {
            texts.push(text.trim());
        } else if let Some(data) = part.get("inlineData").or_else(|| part.get("inline_data")) {
            let mime = data["mimeType"]
                .as_str()
                .or_else(|| data["mime_type"].as_str())
                .context("缺少 inlineData.mimeType")?;
            if !mime.to_ascii_lowercase().starts_with("image/") {
                bail!("inlineData 仅支持图片");
            }
            let encoded = data["data"].as_str().context("缺少 inlineData.data")?;
            request.images.push(format!("data:{mime};base64,{encoded}"));
        } else if let Some(data) = part.get("fileData").or_else(|| part.get("file_data")) {
            let mime = data["mimeType"]
                .as_str()
                .or_else(|| data["mime_type"].as_str())
                .unwrap_or("");
            if !mime.is_empty() && !mime.to_ascii_lowercase().starts_with("image/") {
                bail!("fileData 仅支持图片");
            }
            request.images.push(
                data["fileUri"]
                    .as_str()
                    .or_else(|| data["file_uri"].as_str())
                    .context("缺少 fileData.fileUri")?
                    .into(),
            );
        }
    }
    request.prompt = sanitize(&texts.join("\n"));
    let instruction = body
        .get("systemInstruction")
        .or_else(|| body.get("system_instruction"))
        .unwrap_or(&Value::Null);
    let system = instruction["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| p["text"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let lower = system.to_ascii_lowercase();
    if system.chars().count() <= 1200
        && ![
            "<tools>",
            "</tools>",
            "function calling ai model",
            "function signatures",
            "\"$schema\"",
            "\"additionalproperties\"",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
    {
        let system = sanitize(&system);
        if !system.is_empty() {
            request.prompt = format!("{system}\n\n{}", request.prompt).trim().into();
        }
    }
    Ok(request)
}

/// Preserve image-edit conversations: use the newest downloadable assistant image.
/// Failed historical links are skipped, as in the Python implementation.
pub async fn append_history(
    request: &mut GenerationRequest,
    body: &Value,
    settings: &Settings,
) -> Result<()> {
    let model = catalog::model(&resolver::resolve(&request.model, body))?;
    if model["config"]["type"] != "image" {
        return Ok(());
    }
    let Some(messages) = body["messages"].as_array().filter(|m| m.len() > 1) else {
        return Ok(());
    };
    static IMAGE: OnceLock<Regex> = OnceLock::new();
    let image = IMAGE.get_or_init(|| Regex::new(r"!\[.*?\]\((.*?)\)").unwrap());
    for message in messages[..messages.len() - 1].iter().rev() {
        if message["role"] != "assistant" {
            continue;
        }
        let Some(content) = message["content"].as_str() else {
            continue;
        };
        let urls: Vec<_> = image
            .captures_iter(content)
            .map(|c| c[1].to_owned())
            .collect();
        for url in urls
            .into_iter()
            .rev()
            .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
        {
            if let Ok((bytes, mime)) = generation::load_image(&url, settings).await {
                request
                    .images
                    .insert(0, format!("data:{mime};base64,{}", STANDARD.encode(bytes)));
                return Ok(());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[tokio::test]
    #[ignore = "requires the local Postman media simulation and FLOW2API_MEDIA_SIM_URL"]
    async fn conversation_history_transport_simulation() -> Result<()> {
        let base = std::env::var("FLOW2API_MEDIA_SIM_URL")?;
        let body = json!({"model":"gemini-3.1-flash-image", "messages":[
            {"role":"assistant", "content":format!("![old]({base}/image/error)")},
            {"role":"assistant", "content":format!("![ready]({base}/image/success)\n![expired]({base}/image/error)")},
            {"role":"user", "content":"把背景改成蓝色"}
        ]});
        let mut request = openai(&body)?;
        let settings = Settings::default();
        append_history(&mut request, &body, &settings).await?;
        resolver::prepare(&mut request, &body, &settings).await?;
        assert_eq!(request.images.len(), 1);
        assert!(request.images[0].starts_with("data:image/png;base64,"));
        assert_eq!(request.model, "gemini-3.1-flash-image-square");
        assert_eq!(request.prompt, "把背景改成蓝色");
        request.model = "veo_3_1_t2v_fast".into();
        request.images.clear();
        append_history(&mut request, &body, &settings).await?;
        assert!(request.images.is_empty());
        Ok(())
    }
    #[test]
    fn current_openai_turn_does_not_include_history_or_system_prompt() -> Result<()> {
        let request = openai(&json!({"model":"veo_3_1_t2v_fast", "messages":[
            {"role":"system","content":"tool scaffolding"},
            {"role":"user","content":[{"type":"text","text":"old scene"},{"type":"image_url","image_url":{"url":"https://example.test/old.png"}}]},
            {"role":"assistant","content":"old response"},
            {"role":"user","content":[{"type":"text","text":"continue the shot"},{"type":"image_url","image_url":{"url":"extend://source-id"}}]}
        ],"image":"data:image/png;base64,fixture"}))?;
        assert_eq!(request.prompt, "continue the shot");
        assert_eq!(request.video_id.as_deref(), Some("source-id"));
        assert_eq!(request.images, vec!["data:image/png;base64,fixture"]);
        Ok(())
    }
    #[test]
    fn gemini_latest_user_parts_and_system_scaffolding() -> Result<()> {
        let body = json!({"model":"gemini-3.1-flash-image", "contents":[
            {"role":"user","parts":[{"text":"old scene"}]},
            {"role":"model","parts":[{"inlineData":{"mimeType":"image/png","data":"old"}}]},
            {"parts":[{"text":"You are a function calling AI model.\n<tools>tool schema</tools>\n画一只猫"},{"inline_data":{"mime_type":"image/png","data":"new"}}]}
        ],"systemInstruction":{"parts":[{"text":"Function signatures <tools>ignored</tools>"}]}});
        let request = openai(&body)?;
        assert_eq!(request.prompt, "画一只猫");
        assert_eq!(request.images, vec!["data:image/png;base64,new"]);
        let request = gemini(
            "gemini-3.1-flash-image",
            &json!({"contents":[{"parts":[{"text":"猫"}]}],"system_instruction":{"parts":[{"text":"水彩风格"}]}}),
        )?;
        assert_eq!(request.prompt, "水彩风格\n\n猫");
        assert!(gemini(
            "veo",
            &json!({"contents":[{"parts":[{"inlineData":{"mimeType":"video/mp4","data":"bad"}}]}]})
        )
        .is_err());
        assert!(openai(&json!({"model":"image","messages":[]})).is_err());
        Ok(())
    }
}
