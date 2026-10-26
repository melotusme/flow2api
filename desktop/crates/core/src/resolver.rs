//! Compatibility with the archived Python model resolver.
use crate::{
    catalog,
    model::{GenerationRequest, Settings},
};
use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine};
use image::ImageDecoder;
use serde_json::{json, Value};
use std::sync::OnceLock;

fn tables() -> &'static Value {
    static TABLES: OnceLock<Value> = OnceLock::new();
    TABLES.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/model-aliases.json"))
            .expect("legacy resolver tables")
    })
}
pub fn public_aliases() -> Value {
    let tables = tables();
    let mut aliases = vec![];
    for (alias, base) in tables["IMAGE_BASE_MODELS"].as_object().unwrap() {
        let base = base.as_str().unwrap();
        let aspects = tables["MODEL_SUPPORTED_ASPECTS"][base]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let sizes = tables["MODEL_SUPPORTED_SIZES"][base]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let description = format!(
            "Image generation (alias) - aspects: {aspects}{}",
            if sizes.is_empty() {
                String::new()
            } else {
                format!("; sizes: {sizes}")
            }
        );
        aliases.push(json!({"id":alias,"object":"model","owned_by":"flow2api","description":description,"is_alias":true}));
    }
    for alias in tables["VIDEO_BASE_MODELS"].as_object().unwrap().keys() {
        aliases.push(json!({"id":alias,"object":"model","owned_by":"flow2api","description":"Video generation (alias) - supports landscape/portrait via generationConfig","is_alias":true}));
    }
    json!({"object":"list","data":aliases})
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
    resolve_with_dimensions(id, body, None)
}
fn closest_aspect(width: u32, height: u32, video: bool) -> Option<String> {
    if width == 0 || height == 0 {
        return None;
    }
    let ratio = f64::from(width) / f64::from(height);
    let inferred = [
        ("landscape", 16. / 9.),
        ("portrait", 9. / 16.),
        ("square", 1.),
        ("four-three", 4. / 3.),
        ("three-four", 3. / 4.),
    ]
    .into_iter()
    .min_by(|a, b| (ratio - a.1).abs().total_cmp(&(ratio - b.1).abs()))?
    .0;
    Some(
        if video && !["landscape", "portrait"].contains(&inferred) {
            if height > width {
                "portrait"
            } else {
                "landscape"
            }
        } else {
            inferred
        }
        .into(),
    )
}
fn oriented_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut decoder = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_decoder()
        .ok()?;
    let (width, height) = decoder.dimensions();
    let orientation = decoder.orientation().ok()?.to_exif();
    Some(if (5..=8).contains(&orientation) {
        (height, width)
    } else {
        (width, height)
    })
}
pub async fn prepare(
    request: &mut GenerationRequest,
    body: &Value,
    settings: &Settings,
) -> Result<()> {
    let id = request.model.clone();
    let infer = parameters(body).0.is_none()
        && (tables()["IMAGE_BASE_MODELS"].get(&id).is_some()
            || tables()["VIDEO_BASE_MODELS"].get(&id).is_some());
    request.model = resolve(&id, body);
    crate::generation::validate(request)?;
    let dimensions = if infer {
        if let Some(first) = request.images.first_mut() {
            let (bytes, mime) = crate::generation::load_image(first, settings).await?;
            let dimensions = oriented_dimensions(&bytes);
            // Cache the exact downloaded bytes in the task; generation uploads them
            // once rather than fetching a potentially changing URL a second time.
            *first = format!("data:{mime};base64,{}", STANDARD.encode(&bytes));
            dimensions
        } else {
            None
        }
    } else {
        None
    };
    request.model = resolve_with_dimensions(&id, body, dimensions);
    Ok(())
}
fn resolve_with_dimensions(id: &str, body: &Value, dimensions: Option<(u32, u32)>) -> String {
    let tables = tables();
    let (mut ratio, size) = parameters(body);
    if ratio.is_none() {
        ratio = dimensions.and_then(|(width, height)| {
            closest_aspect(width, height, tables["VIDEO_BASE_MODELS"].get(id).is_some())
        });
    }
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
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::new(width, height))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }
    #[tokio::test]
    async fn reference_dimensions_follow_alias_without_overriding_explicit_ratio() -> Result<()> {
        let data = format!("data:image/png;base64,{}", STANDARD.encode(png(30, 40)));
        let mut request = GenerationRequest {
            model: "gemini-3.1-flash-image".into(),
            prompt: "fixture".into(),
            images: vec![data.clone()],
            account_id: None,
            video_id: None,
        };
        prepare(&mut request, &json!({}), &Settings::default()).await?;
        assert_eq!(request.model, "gemini-3.1-flash-image-three-four");
        request.model = "gemini-3.1-flash-image".into();
        prepare(
            &mut request,
            &json!({"generationConfig":{"imageConfig":{"aspectRatio":"1:1"}}}),
            &Settings::default(),
        )
        .await?;
        assert_eq!(request.model, "gemini-3.1-flash-image-square");
        request.model = "veo_3_1_i2v_s_fast_fl".into();
        prepare(&mut request, &json!({}), &Settings::default()).await?;
        assert_eq!(request.model, "veo_3_1_i2v_s_fast_portrait_fl");
        assert_eq!(request.images[0], data);
        Ok(())
    }
    #[test]
    fn jpeg_exif_orientation_swaps_dimensions_before_inference() {
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode_image(&image::DynamicImage::ImageRgb8(image::RgbImage::new(8, 4)))
            .unwrap();
        assert_eq!(oriented_dimensions(&jpeg), Some((8, 4)));
        let exif = b"Exif\0\0II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
        let mut segment = vec![0xff, 0xe1];
        segment.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        segment.extend_from_slice(exif);
        jpeg.splice(2..2, segment);
        assert_eq!(oriented_dimensions(&jpeg), Some((4, 8)));
        assert_eq!(
            resolve_with_dimensions(
                "gemini-3.1-flash-image",
                &json!({}),
                oriented_dimensions(&jpeg)
            ),
            "gemini-3.1-flash-image-portrait"
        );
        assert_eq!(oriented_dimensions(b"<html>not an image</html>"), None);
    }
    #[tokio::test]
    #[ignore = "requires the local Postman media simulation and FLOW2API_MEDIA_SIM_URL"]
    async fn reference_transport_simulation() -> Result<()> {
        let base = std::env::var("FLOW2API_MEDIA_SIM_URL")?;
        let url = format!("{base}/image/once/{}", uuid::Uuid::new_v4());
        let mut request = GenerationRequest {
            model: "gemini-3.1-flash-image".into(),
            prompt: "fixture".into(),
            images: vec![url.clone()],
            account_id: None,
            video_id: None,
        };
        let settings = Settings::default();
        prepare(&mut request, &json!({}), &settings).await?;
        assert_eq!(request.model, "gemini-3.1-flash-image-square");
        assert!(request.images[0].starts_with("data:image/png;base64,"));
        let (bytes, _) = crate::generation::load_image(&request.images[0], &settings).await?;
        assert_eq!(oriented_dimensions(&bytes), Some((1, 1)));
        // Prove a second HTTP read would fail, while the generation upload source above succeeds.
        assert!(crate::generation::load_image(&url, &settings)
            .await
            .is_err());
        let error =
            crate::generation::load_image(&format!("{base}/image/chunked-oversize"), &settings)
                .await
                .err()
                .expect("oversize must fail");
        assert!(error.to_string().contains("20 MB"));
        assert!(
            crate::generation::load_image(&format!("{base}/image/html"), &settings)
                .await
                .is_err()
        );
        Ok(())
    }
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
