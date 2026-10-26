//! Compatibility with the archived Python model resolver.
use crate::catalog;
use serde_json::Value;
use std::sync::OnceLock;

fn tables() -> &'static Value {
    static TABLES: OnceLock<Value> = OnceLock::new();
    TABLES.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/model-aliases.json"))
            .expect("legacy resolver tables")
    })
}
fn field<'a>(value: &'a Value, names: &[&str]) -> Option<&'a str> {
    names.iter().find_map(|name| {
        value[*name]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    })
}
fn aspect(value: &str) -> String {
    let token = value
        .replace(['：', '/', 'x', 'X'], ":")
        .replace(' ', "")
        .to_lowercase();
    tables()["ASPECT_RATIO_MAP"][&token]
        .as_str()
        .unwrap_or(&token)
        .to_owned()
}
fn resolution(value: &str) -> String {
    value.replace(' ', "").to_lowercase()
}
fn dimensions(value: &str) -> Option<String> {
    let (w, h) = value.split_once(['x', 'X'])?;
    if !(2..=5).contains(&w.trim().len()) || !(2..=5).contains(&h.trim().len()) {
        return None;
    }
    let (w, h) = (w.trim().parse::<f64>().ok()?, h.trim().parse::<f64>().ok()?);
    if w <= 0. || h <= 0. {
        return None;
    }
    [
        ("landscape", 16. / 9.),
        ("portrait", 9. / 16.),
        ("square", 1.),
        ("four-three", 4. / 3.),
        ("three-four", 3. / 4.),
    ]
    .into_iter()
    .min_by(|a, b| ((w / h) - a.1).abs().total_cmp(&((w / h) - b.1).abs()))
    .map(|v| v.0.to_owned())
}
fn quality(value: &str) -> Option<String> {
    let token = resolution(value);
    if tables()["IMAGE_SIZE_MAP"].get(&token).is_some() {
        return Some(token);
    }
    tables()["OPENAI_QUALITY_MAP"][&token]
        .as_str()
        .map(str::to_owned)
}
fn parameters(body: &Value) -> (Option<String>, Option<String>) {
    let mut ratio = None;
    let mut size = None;
    let generation = &body["generationConfig"];
    let extra = body
        .get("extra_body")
        .or_else(|| body.get("extraBody"))
        .unwrap_or(&Value::Null);
    for config in [generation, &extra["generationConfig"], body] {
        let image = config
            .get("imageConfig")
            .or_else(|| config.get("image_config"))
            .unwrap_or(&Value::Null);
        for object in [image, config] {
            if ratio.is_none() {
                ratio = field(object, &["aspectRatio", "aspect_ratio", "aspect"])
                    .map(aspect)
                    .or_else(|| field(object, &["size"]).and_then(dimensions));
            }
            if size.is_none() {
                size = field(object, &["imageSize", "image_size", "resolution"])
                    .map(resolution)
                    .or_else(|| {
                        field(object, &["quality", "imageQuality", "image_quality"])
                            .and_then(quality)
                    });
            }
        }
    }
    (ratio, size)
}
pub fn resolve(id: &str, body: &Value) -> String {
    let tables = tables();
    let (ratio, size) = parameters(body);
    if let Some(base) = tables["IMAGE_BASE_MODELS"][id].as_str() {
        let ratio = ratio
            .as_deref()
            .filter(|r| {
                tables["MODEL_SUPPORTED_ASPECTS"][base]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(r)))
            })
            .unwrap_or("landscape");
        let mut resolved = format!("{base}-{ratio}");
        if let Some(size) = size.as_deref().filter(|s| {
            tables["MODEL_SUPPORTED_SIZES"][base]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(s)))
        }) {
            resolved.push('-');
            resolved.push_str(size);
        }
        return if catalog::catalog().get(&resolved).is_some() {
            resolved
        } else {
            id.into()
        };
    }
    if tables["VIDEO_BASE_MODELS"].get(id).is_some() {
        let mut base = id.to_owned();
        if let Some(size) = size.filter(|s| ["4k", "1080p"].contains(&s.as_str())) {
            let candidate = format!("{id}_{size}");
            if tables["VIDEO_BASE_MODELS"].get(&candidate).is_some() {
                base = candidate;
            }
        }
        let ratio = ratio
            .as_deref()
            .filter(|r| ["landscape", "portrait"].contains(r))
            .unwrap_or("landscape");
        if let Some(resolved) = tables["VIDEO_BASE_MODELS"][&base][ratio]
            .as_str()
            .filter(|v| catalog::catalog().get(*v).is_some())
        {
            return resolved.into();
        }
    }
    id.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn legacy_image_defaults_and_config_precedence() {
        assert_eq!(
            resolve("gemini-3.1-flash-image", &json!({})),
            "gemini-3.1-flash-image-landscape"
        );
        assert_eq!(
            resolve(
                "gemini-3.1-flash-image",
                &json!({"generationConfig":{"imageConfig":{"aspectRatio":"9：16","imageSize":"2K"}},"size":"1024x1024","quality":"high"})
            ),
            "gemini-3.1-flash-image-portrait-2k"
        );
        assert_eq!(
            resolve(
                "gemini-3.0-pro-image",
                &json!({"extra_body":{"generationConfig":{"image_config":{"aspect_ratio":"4/3","image_size":"4K"}}}})
            ),
            "gemini-3.0-pro-image-four-three-4k"
        );
        assert_eq!(
            resolve(
                "gemini-3.1-flash-image",
                &json!({"size":"1024x1792","quality":"hd"})
            ),
            "gemini-3.1-flash-image-portrait-4k"
        );
        assert_eq!(
            resolve(
                "gemini-3.1-flash-image-square",
                &json!({"aspectRatio":"9:16"})
            ),
            "gemini-3.1-flash-image-square"
        );
    }
    #[test]
    fn all_legacy_video_aliases_preserve_orientation() {
        for (alias, variants) in tables()["VIDEO_BASE_MODELS"].as_object().unwrap() {
            for (orientation, expected) in variants.as_object().unwrap() {
                if catalog::catalog().get(expected.as_str().unwrap()).is_some() {
                    assert_eq!(
                        resolve(alias, &json!({"aspectRatio":orientation})),
                        expected.as_str().unwrap(),
                        "{alias} {orientation}"
                    );
                }
            }
        }
        assert_eq!(resolve("unknown", &json!({})), "unknown");
    }
}
