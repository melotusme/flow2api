//! Port of the archived Flow batchexecute protocol. Chrome owns HTTP/TLS and cookies.
use crate::browser::Session;
use anyhow::{bail, Context, Result};
use regex::Regex;
use serde_json::{json, Value};

pub fn context(project: &str, captcha: &str) -> Value {
    json!([
        null,
        22,
        null,
        null,
        null,
        project,
        null,
        null,
        null,
        null,
        [captcha, 1]
    ])
}
pub fn image_argument(
    project: &str,
    prompt: &str,
    model: &str,
    aspect: &str,
    captcha: &str,
    references: &[String],
) -> Value {
    let session = uuid::Uuid::new_v4().to_string().to_uppercase();
    let aspect = match aspect {
        "IMAGE_ASPECT_RATIO_SQUARE" => 1,
        "IMAGE_ASPECT_RATIO_PORTRAIT" => 2,
        "IMAGE_ASPECT_RATIO_LANDSCAPE_FOUR_THREE" => 4,
        "IMAGE_ASPECT_RATIO_PORTRAIT_THREE_FOUR" => 5,
        _ => 3,
    };
    let refs = if references.is_empty() {
        Value::Null
    } else {
        json!(references
            .iter()
            .map(|id| json!([id, null, null, null, 1]))
            .collect::<Vec<_>>())
    };
    let seed = (uuid::Uuid::new_v4().as_u128() % 2_147_483_646 + 1) as u32;
    json!([
        null,
        [[
            null,
            null,
            refs,
            seed,
            aspect,
            model,
            null,
            context(project, captcha),
            [[[prompt]]],
            null,
            null,
            null,
            session,
            uuid::Uuid::new_v4().to_string().to_uppercase()
        ]],
        1,
        context(project, captcha),
        [session]
    ])
}

pub fn video_argument(
    project: &str,
    prompt: &str,
    key: &str,
    aspect: &str,
    captcha: &str,
    mode: &str,
    references: &[String],
    video_id: Option<&str>,
    resolution: Option<&str>,
) -> Result<(&'static str, Value)> {
    let aspect = if aspect.contains("PORTRAIT") {
        1
    } else if aspect.contains("SQUARE") {
        0
    } else {
        2
    };
    let res = resolution.map(|v| {
        if v.contains("4K") {
            4
        } else if v.contains("1080") {
            3
        } else if v.contains("720") {
            2
        } else {
            1
        }
    });
    let prompt = json!([null, null, [[[prompt]]]]);
    let meta = json!([
        null,
        null,
        null,
        null,
        uuid::Uuid::new_v4().to_string().to_uppercase(),
        uuid::Uuid::new_v4().to_string().to_uppercase()
    ]);
    let reference = |index: usize| -> Result<Value> {
        Ok(json!([
            null,
            references.get(index).context("缺少首帧或尾帧参考图")?
        ]))
    };
    let (rpc, mut request) = match mode {
        "references" => (
            "MZZa6b",
            json!([
                prompt,
                references
                    .iter()
                    .map(|id| json!([null, id]))
                    .collect::<Vec<_>>(),
                key,
                aspect,
                null,
                meta,
                null,
                null,
                null,
                null,
                null,
                res
            ]),
        ),
        "start_end" => (
            "nprQif",
            json!([
                prompt,
                key,
                aspect,
                null,
                reference(0)?,
                reference(1)?,
                meta,
                null,
                null
            ]),
        ),
        "start" => (
            "eb1hJf",
            json!([
                prompt,
                key,
                aspect,
                null,
                reference(0)?,
                meta,
                null,
                null,
                null,
                res
            ]),
        ),
        "extend" => (
            "fZytfe",
            json!([
                [null, video_id.context("缺少源视频 ID")?],
                prompt,
                key,
                aspect,
                null,
                meta,
                null,
                null
            ]),
        ),
        "upsample" => (
            "p0UkFb",
            json!([
                [null, video_id.context("缺少源视频 ID")?],
                null,
                aspect,
                42,
                meta,
                null,
                res,
                null
            ]),
        ),
        "text" => (
            "YhhmEf",
            json!([prompt, key, aspect, null, meta, null, null, res]),
        ),
        _ => bail!("未知视频工作流"),
    };
    if mode == "upsample" && !key.is_empty() {
        let items = request.as_array_mut().unwrap();
        items.resize(31, Value::Null);
        items.push(json!(key));
    }
    Ok((
        rpc,
        json!([
            [request],
            context(project, captcha),
            [uuid::Uuid::new_v4().to_string().to_uppercase(), 2]
        ]),
    ))
}

pub fn parse_rpc(text: &str, rpc: &str) -> Result<Value> {
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('['))
    {
        let Ok(frame) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        for item in frame.as_array().into_iter().flatten() {
            if item[0] == "er" {
                bail!("Flow RPC {rpc} 拒绝请求，请核对登录、权限和额度");
            }
            if item[0] == "wrb.fr" && item[1] == rpc {
                if let Some(code) = error_code(&item[5]) {
                    bail!("Flow RPC 拒绝请求：{code}");
                }
                let data = item[2].as_str().context("Flow RPC 返回错误或缺少数据")?;
                return serde_json::from_str(data).context("Flow RPC 数据格式错误");
            }
        }
    }
    bail!("Flow RPC {rpc} 未返回有效数据；请检查登录状态")
}
pub fn error_code(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if s.starts_with("PUBLIC_ERROR_") || s.starts_with("ERROR_") => {
            Some(s.clone())
        }
        Value::Array(a) => a.iter().find_map(error_code),
        Value::Object(o) => o.values().find_map(error_code),
        _ => None,
    }
}
pub fn credits(value: &Value) -> Result<i64> {
    value
        .as_array()
        .and_then(|a| a.iter().find_map(Value::as_i64))
        .filter(|v| *v >= 0)
        .context("Flow 未返回有效余额，登录校验没有通过")
}
pub fn media_url(value: &Value, kind: &str) -> Option<String> {
    match value {
        Value::String(s) if s.starts_with("https://") && s.contains(&format!("/{kind}/")) => {
            Some(s.clone())
        }
        Value::Array(a) => a.iter().find_map(|v| media_url(v, kind)),
        Value::Object(o) => o.values().find_map(|v| media_url(v, kind)),
        _ => None,
    }
}
pub fn find_uuid(value: &Value, excluded: &[&str]) -> Option<String> {
    match value {
        Value::String(s) if uuid::Uuid::parse_str(s).is_ok() && !excluded.contains(&s.as_str()) => {
            Some(s.clone())
        }
        Value::Array(a) => a.iter().find_map(|v| find_uuid(v, excluded)),
        Value::Object(o) => o.values().find_map(|v| find_uuid(v, excluded)),
        _ => None,
    }
}
pub fn video_records<'a>(value: &'a Value, project: Option<&str>) -> Vec<&'a Value> {
    if value.is_array()
        && value[0]
            .as_str()
            .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
        && value[2]
            .as_str()
            .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
        && project.is_none_or(|id| value[1] == id)
    {
        return vec![value];
    }
    match value {
        Value::Array(a) => a.iter().flat_map(|v| video_records(v, project)).collect(),
        Value::Object(o) => o.values().flat_map(|v| video_records(v, project)).collect(),
        _ => vec![],
    }
}

pub async fn rpc(
    session: &mut Session,
    rpc: &str,
    argument: Value,
    project: Option<&str>,
) -> Result<Value> {
    let path = project
        .map(|id| format!("/project/{id}"))
        .unwrap_or_else(|| "/projects".into());
    let bootstrap = session
        .fetch(
            &format!("https://flow.google.com{path}"),
            "GET",
            None,
            "text/plain",
        )
        .await?;
    if bootstrap["status"] != 200 {
        bail!("Flow 页面认证失败，请重新登录同步");
    }
    let text = bootstrap["text"].as_str().unwrap_or_default();
    let bootloader = Regex::new(r"boq_labs-ai-sandbox-frontend_[A-Za-z0-9_.-]+")?
        .find(text)
        .context("Flow 启动元数据缺失")?
        .as_str();
    let sid_regex = Regex::new(r#""FdrFJe"\s*:\s*"(-?[0-9]+)""#)?;
    let sid = sid_regex.captures(text).context("Flow 会话元数据缺失")?;
    let mut url =
        url::Url::parse("https://flow.google.com/_/AiSandboxAngularFrontend/data/batchexecute")?;
    url.query_pairs_mut().extend_pairs([
        ("rpcids", rpc),
        ("source-path", &path),
        ("bl", bootloader),
        ("f.sid", sid.get(1).unwrap().as_str()),
        ("hl", "en"),
        ("_reqid", "45123"),
        ("rt", "c"),
    ]);
    let request = json!([[[rpc, argument.to_string(), null, "generic"]]]).to_string();
    let at = Regex::new(r#""SNlM0e"\s*:\s*"((?:\\.|[^"\\])+)""#)?
        .captures(text)
        .map(|v| serde_json::from_str::<String>(&format!("\"{}\"", &v[1])))
        .transpose()?
        .unwrap_or_default();
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("f.req", &request)
        .append_pair("at", &at)
        .finish();
    let response = session
        .fetch(
            url.as_str(),
            "POST",
            Some(&body),
            "application/x-www-form-urlencoded;charset=UTF-8",
        )
        .await?;
    if response["status"] != 200 {
        bail!(
            "Flow RPC {rpc} HTTP 错误 {}；请求可能已提交，请先核对 Flow 结果",
            response["status"]
        );
    }
    let text = response["text"].as_str().context("Flow RPC 缺少响应")?;
    // XSRF rotation is an explicit pre-submission rejection; repeat only this rejected RPC.
    if let Some(captures) = Regex::new(r#"\["xsrf"\s*,\s*"((?:\\.|[^"\\])*)""#)?.captures(text) {
        let at: String = serde_json::from_str(&format!("\"{}\"", &captures[1]))?;
        let body = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("f.req", &request)
            .append_pair("at", &at)
            .finish();
        let response = session
            .fetch(
                url.as_str(),
                "POST",
                Some(&body),
                "application/x-www-form-urlencoded;charset=UTF-8",
            )
            .await?;
        if response["status"] != 200 {
            bail!("Flow XSRF 更新后的请求失败");
        }
        return parse_rpc(response["text"].as_str().unwrap_or_default(), rpc);
    }
    parse_rpc(text, rpc)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_real_batchexecute_shape_and_reject_errors() -> Result<()> {
        let frame = json!([[
            "wrb.fr",
            "ogiZ0b",
            "[[[\"id\",\"https://example.com/image/id\"]]]",
            null,
            null,
            null
        ]]);
        assert_eq!(
            parse_rpc(&format!(")]}}'\n\n123\n{frame}"), "ogiZ0b")?[0][0][0],
            "id"
        );
        assert!(parse_rpc("[[\"er\",\"ogiZ0b\",null,null,null,401]]", "ogiZ0b").is_err());
        assert!(parse_rpc(
            "[[\"wrb.fr\",\"ogiZ0b\",null,null,null,[\"PUBLIC_ERROR_UNSAFE_GENERATION\"]]]",
            "ogiZ0b"
        )
        .is_err());
        Ok(())
    }
    #[test]
    fn image_arguments_keep_model_prompt_aspect_and_references() {
        let argument = image_argument(
            "project",
            "prompt",
            "HARBOR_SEAL",
            "IMAGE_ASPECT_RATIO_SQUARE",
            "captcha",
            &["ref".into()],
        );
        assert_eq!(argument[1][0][5], "HARBOR_SEAL");
        assert_eq!(argument[1][0][4], 1);
        assert_eq!(argument[1][0][8], json!([[["prompt"]]]));
        assert_eq!(argument[1][0][2][0][0], "ref");
        assert_eq!(argument[3][10][0], "captcha");
    }
}
