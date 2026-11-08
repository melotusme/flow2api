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

pub fn rpc_label(rpc: &str) -> &'static str {
    match rpc {
        "nzlxg" => "查询账号余额",
        "jHPbke" => "创建 Flow 项目",
        "maseQ" => "上传参考图",
        "ogiZ0b" => "提交图片生成",
        "SPrCad" => "图片高清放大",
        "YhhmEf" => "提交文生视频",
        "eb1hJf" => "提交图生视频（首帧）",
        "nprQif" => "提交图生视频（首尾帧）",
        "MZZa6b" => "提交多参考图视频",
        "fZytfe" => "提交视频续写",
        "p0UkFb" => "视频高清放大",
        "jwpduf" => "轮询视频生成状态",
        "as29s" => "查询视频媒体地址",
        _ => "调用 Flow 接口",
    }
}

pub fn explain_error_code(code: &str) -> (&'static str, &'static str) {
    match code {
        "PUBLIC_ERROR_MODEL_ACCESS_DENIED" | "ERROR_MODEL_ACCESS_DENIED" => (
            "当前 Google 账号无该模型权限",
            "该模型（如 Veo 3.1 高清／专业版）通常需要 Google AI Ultra 等高阶订阅。请将有权限账号设为高优先级、关闭此账号的视频生成开关，或改用 Fast 模型",
        ),
        "PUBLIC_ERROR_UNSAFE_GENERATION"
        | "PUBLIC_ERROR_SAFETY_FILTER"
        | "ERROR_UNSAFE_GENERATION" => (
            "提示词或参考图触发了安全审核拦截",
            "请调整提示词措辞或更换参考图后再试",
        ),
        "PUBLIC_ERROR_PROMINENT_PEOPLE_FILTER_FAILED" | "PUBLIC_ERROR_PERSON_GENERATION" => (
            "参考图或提示词触发了人物保护策略",
            "请更换不包含特定公众人物面部特征的参考图或描述",
        ),
        "PUBLIC_ERROR_QUOTA_EXCEEDED"
        | "PUBLIC_ERROR_INSUFFICIENT_CREDITS"
        | "ERROR_QUOTA_EXCEEDED"
        | "ERROR_RESOURCE_EXHAUSTED" => (
            "当前账号 AI 积分不足或达到配额上限",
            "请在账号页同步核对剩余积分、临时禁用该账号，或切换到积分充足的账号",
        ),
        "PUBLIC_ERROR_RATE_LIMIT_EXCEEDED" | "PUBLIC_ERROR_TOO_MANY_REQUESTS" => (
            "当前账号请求过于频繁，触发上游限流",
            "建议将该账号临时禁用 15–60 分钟，或配置多个账号分摊并发",
        ),
        "PUBLIC_ERROR_INVALID_ARGUMENT" | "PUBLIC_ERROR_BAD_REQUEST" => (
            "请求参数或参考图不被上游模型接受",
            "请检查参考图格式、尺寸或源视频 ID 是否有效",
        ),
        "PUBLIC_ERROR_UNAUTHENTICATED" | "ERROR_UNAUTHENTICATED" => (
            "Google Flow 登录状态已失效",
            "请在账号管理页重新连接或同步该账号",
        ),
        _ => (
            "上游服务拒绝了本次生成请求",
            "请检查提示词、参考图与账号订阅权限，或切换其他账号重试",
        ),
    }
}

fn grpc_status_code(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64().filter(|c| (1..=16).contains(c)),
        Value::Array(a) => a.first().and_then(grpc_status_code),
        _ => None,
    }
}

pub fn parse_rpc(text: &str, rpc: &str) -> Result<Value> {
    let action = rpc_label(rpc);
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
                bail!("{action}被拒绝（Flow RPC {rpc}），请核对登录状态、订阅权限和积分余额");
            }
            if item[0] == "wrb.fr" && item[1] == rpc {
                if let Some(code) = error_code(&item[5]) {
                    let (summary, advice) = explain_error_code(&code);
                    bail!("{action}被拒绝：{summary}（{code}）。{advice}");
                }
                let Some(data) = item[2].as_str() else {
                    let detail = match grpc_status_code(&item[5]) {
                        Some(3) => {
                            format!("状态码 3 INVALID_ARGUMENT：{action}时参数或模型键不被接受")
                        }
                        Some(5) => format!(
                            "状态码 5 NOT_FOUND：{action}时当前账号不支持该模型键或项目不存在"
                        ),
                        Some(7) => format!(
                            "状态码 7 PERMISSION_DENIED：{action}时当前账号无此模型或功能权限"
                        ),
                        Some(8) => format!(
                            "状态码 8 RESOURCE_EXHAUSTED：{action}时当前账号积分不足或触发限流"
                        ),
                        Some(16) => {
                            "状态码 16 UNAUTHENTICATED：Google 登录状态已失效，请重新连接或同步账号"
                                .into()
                        }
                        Some(code) => {
                            format!("状态码 {code}：{action}时上游返回异常，请稍后重试或切换账号")
                        }
                        None => format!(
                            "{action}未返回数据，可能是当前账号无该模型权限、积分不足或项目已失效"
                        ),
                    };
                    bail!("Flow RPC 返回错误或缺少数据（{detail}）");
                };
                return serde_json::from_str(data).context("Flow RPC 数据格式错误");
            }
        }
    }
    bail!("{action}未返回有效数据（Flow RPC {rpc}）；请检查登录状态或重新连接账号")
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
