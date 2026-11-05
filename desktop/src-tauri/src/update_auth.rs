//! Read-only adapters for the user's existing GitHub login. Secrets never cross IPC.
//!
//! Desktop's store contract is from desktop/desktop app/src/lib/auth.ts and
//! stores/token-store.ts; keytar uses this exact service on macOS and the
//! service/account target with a UTF-8 blob on Windows. No refresh token is used
//! or rotated here: GitHub Desktop remains the owner of its login.

use serde_json::Value;
use std::future::Future;

const DESKTOP_SERVICE: &str = "GitHub - https://api.github.com";
const RELEASE_URL: &str =
    "https://api.github.com/repos/melotusme/flow2api-releases/releases/latest";

#[derive(Clone, Copy)]
pub enum Source {
    Cli,
    Desktop,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cli => "GitHub CLI",
            Self::Desktop => "GitHub Desktop",
        }
    }
}

// Deliberately neither Debug nor Serialize.
pub struct Credential {
    pub token: String,
    pub source: Source,
}

enum ValidationError {
    Rejected(&'static str),
    Fatal(String),
}

pub async fn release(
    client: &reqwest::Client,
    interactive: bool,
) -> Result<(Credential, Value), String> {
    select(
        |source| async move {
            tauri::async_runtime::spawn_blocking(move || match source {
                Source::Cli => cli_tokens(),
                Source::Desktop => desktop_tokens(interactive),
            })
            .await
            .map_err(|_| "读取登录状态失败，请重试。".to_string())?
        },
        |token| async move {
            let response = client
                .get(RELEASE_URL)
                .bearer_auth(token)
                .header(reqwest::header::ACCEPT, "application/vnd.github+json")
                .send()
                .await
                .map_err(|_| {
                    ValidationError::Fatal("GitHub 更新连接失败或超时，请检查网络。".into())
                })?;
            match response.status().as_u16() {
                200 => response
                    .json()
                    .await
                    .map_err(|_| ValidationError::Fatal("GitHub Release 元数据格式异常。".into())),
                401 => Err(ValidationError::Rejected(
                    "登录已失效，请在该客户端重新登录。",
                )),
                403 | 404 => Err(ValidationError::Rejected(
                    "无法读取私有发行仓库，请确认当前账号有仓库权限。",
                )),
                code => Err(ValidationError::Fatal(format!(
                    "GitHub 更新请求失败（HTTP {code}），请稍后重试。"
                ))),
            }
        },
    )
    .await
}

async fn select<L, LF, V, VF>(mut load: L, mut validate: V) -> Result<(Credential, Value), String>
where
    L: FnMut(Source) -> LF,
    LF: Future<Output = Result<Vec<String>, String>>,
    V: FnMut(String) -> VF,
    VF: Future<Output = Result<Value, ValidationError>>,
{
    let mut failures = Vec::new();
    // Avoid an unnecessary Keychain permission request when CLI already works.
    for source in [Source::Cli, Source::Desktop] {
        let tokens = match load(source).await {
            Ok(tokens) if !tokens.is_empty() => tokens,
            Ok(_) => {
                failures.push(format!(
                    "{}：未找到已登录的 GitHub.com 账号。",
                    source.label()
                ));
                continue;
            }
            Err(error) => {
                failures.push(format!("{}：{error}", source.label()));
                continue;
            }
        };
        for token in tokens {
            match validate(token.clone()).await {
                Ok(release) => return Ok((Credential { token, source }, release)),
                Err(ValidationError::Rejected(reason)) => {
                    failures.push(format!("{}：{reason}", source.label()));
                }
                Err(ValidationError::Fatal(error)) => return Err(error),
            }
        }
    }
    Err(format!(
        "无法验证更新登录。{} 可在 GitHub Desktop 登录有权限的账号后重试，或执行 gh auth login。",
        failures.join(" ")
    ))
}

fn cli_tokens() -> Result<Vec<String>, String> {
    let program = gh_program().ok_or("未安装或未找到 gh。")?;
    let output = super::updates::hidden(&program)
        .args(["auth", "token", "--hostname", "github.com"])
        .output()
        .map_err(|_| "无法启动 gh。")?;
    if !output.status.success() {
        return Err("尚未登录，请执行 gh auth login。".into());
    }
    let token = std::str::from_utf8(&output.stdout)
        .ok()
        .and_then(valid_token)
        .ok_or("登录凭据为空或格式异常。")?;
    Ok(vec![token])
}

fn gh_program() -> Option<String> {
    #[cfg(target_os = "windows")]
    let fixed = std::env::var_os("ProgramFiles")
        .map(std::path::PathBuf::from)
        .map(|path| path.join("GitHub CLI/gh.exe"))
        .into_iter()
        .collect::<Vec<_>>();
    #[cfg(not(target_os = "windows"))]
    let fixed = ["/opt/homebrew/bin/gh", "/usr/local/bin/gh", "/usr/bin/gh"]
        .into_iter()
        .map(std::path::PathBuf::from)
        .collect::<Vec<_>>();
    let executable = if cfg!(windows) { "gh.exe" } else { "gh" };
    fixed
        .into_iter()
        .chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|p| p.join(executable)),
        )
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}

fn valid_token(value: &str) -> Option<String> {
    let value = value.trim();
    // Reject malformed data before it can become an HTTP header; never echo it.
    (!value.is_empty()
        && value.len() <= 4096
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_'))
    .then(|| value.to_owned())
}

fn desktop_token(bytes: &[u8]) -> Option<String> {
    let value = std::str::from_utf8(bytes).ok()?;
    if let Some(record) = value.strip_prefix("github-desktop-oauth:") {
        let record: Value = serde_json::from_str(record).ok()?;
        if record["version"] != 1 {
            return None;
        }
        valid_token(record["credential"]["accessToken"].as_str()?)
    } else {
        valid_token(value)
    }
}

#[cfg(target_os = "macos")]
fn desktop_tokens(interactive: bool) -> Result<Vec<String>, String> {
    use core_foundation::{
        base::{CFType, TCFType},
        boolean::CFBoolean,
        data::CFData,
        dictionary::CFDictionary,
        string::{CFString, CFStringRef},
    };
    use security_framework::item::{ItemClass, ItemSearchOptions, Limit};
    use security_framework_sys::{item::*, keychain_item::SecItemCopyMatching};
    extern "C" {
        static kSecUseAuthenticationUIFail: CFStringRef;
    }
    // Legacy macOS Keychains reject ReturnData + MatchLimitAll (errSecParam).
    // Enumerate metadata first, then read each exact service/account separately.
    let accounts = match ItemSearchOptions::new()
        .class(ItemClass::generic_password())
        .service(DESKTOP_SERVICE)
        .load_attributes(true)
        .skip_authenticated_items(!interactive)
        .limit(Limit::All)
        .search()
    {
        Ok(items) => items,
        Err(error) if error.code() == -25300 => return Ok(Vec::new()),
        Err(_) => return Err("系统钥匙串不可读，请解锁后重试。".into()),
    };
    let string = |value| unsafe { CFString::wrap_under_get_rule(value).into_CFType() };
    let account_key = unsafe { CFString::wrap_under_get_rule(kSecAttrAccount).to_string() };
    let mut tokens = Vec::new();
    let mut failure = None;
    for item in accounts {
        let attributes = match item.simplify_dict() {
            Some(v) => v,
            None => continue,
        };
        let account = match attributes.get(&account_key) {
            Some(v) => v,
            None => continue,
        };
        let mut pairs = vec![
            (
                string(unsafe { kSecClass }),
                string(unsafe { kSecClassGenericPassword }),
            ),
            (
                string(unsafe { kSecAttrService }),
                CFString::new(DESKTOP_SERVICE).into_CFType(),
            ),
            (
                string(unsafe { kSecAttrAccount }),
                CFString::new(account).into_CFType(),
            ),
            (
                string(unsafe { kSecReturnData }),
                CFBoolean::true_value().into_CFType(),
            ),
        ];
        // Fail suppresses system UI during automatic checks. Manual checks can
        // request the OS's normal permission; no ACL or login state is modified.
        if !interactive {
            pairs.push((
                string(unsafe { kSecUseAuthenticationUI }),
                string(unsafe { kSecUseAuthenticationUIFail }),
            ));
        }
        let query = CFDictionary::from_CFType_pairs(&pairs);
        let mut result = std::ptr::null();
        let status = unsafe { SecItemCopyMatching(query.as_concrete_TypeRef(), &mut result) };
        if status == 0 && !result.is_null() {
            let result = unsafe { CFType::wrap_under_create_rule(result) };
            if let Some(data) = result.downcast::<CFData>() {
                tokens.extend(desktop_token(data.bytes()));
            }
        } else {
            failure = Some(format!("系统钥匙串拒绝或无法读取登录（OSStatus {status}）；请解锁钥匙串，手动检查更新并允许 Flow2API 读取。"));
        }
    }
    if tokens.is_empty() {
        return Err(
            failure.unwrap_or_else(|| "登录凭据为空或格式不受支持，请在 Desktop 重新登录。".into())
        );
    }
    Ok(tokens)
}

#[cfg(target_os = "windows")]
fn desktop_tokens(_interactive: bool) -> Result<Vec<String>, String> {
    use windows_sys::Win32::{
        Foundation::{GetLastError, ERROR_NOT_FOUND},
        Security::Credentials::{CredEnumerateW, CredFree, CREDENTIALW, CRED_TYPE_GENERIC},
    };
    let filter = format!("{DESKTOP_SERVICE}/*\0")
        .encode_utf16()
        .collect::<Vec<_>>();
    let mut count = 0;
    let mut credentials: *mut *mut CREDENTIALW = std::ptr::null_mut();
    // Only enumerate Desktop's exact GitHub.com service namespace, never all credentials.
    if unsafe { CredEnumerateW(filter.as_ptr(), 0, &mut count, &mut credentials) } == 0 {
        return if unsafe { GetLastError() } == ERROR_NOT_FOUND {
            Ok(Vec::new())
        } else {
            Err("Windows 凭据管理器拒绝或无法读取登录。".into())
        };
    }
    struct Allocation(*mut *mut CREDENTIALW);
    impl Drop for Allocation {
        fn drop(&mut self) {
            unsafe { CredFree(self.0.cast()) };
        }
    }
    let _allocation = Allocation(credentials);
    let mut tokens = Vec::new();
    for i in 0..count as usize {
        let item = unsafe { &**credentials.add(i) };
        if item.Type == CRED_TYPE_GENERIC
            && !item.CredentialBlob.is_null()
            && item.CredentialBlobSize > 0
            && item.CredentialBlobSize <= 16 * 1024
        {
            let bytes = unsafe {
                std::slice::from_raw_parts(item.CredentialBlob, item.CredentialBlobSize as usize)
            };
            if let Some(token) = desktop_token(bytes) {
                tokens.push(token);
            }
        }
    }
    Ok(tokens)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn desktop_tokens(_interactive: bool) -> Result<Vec<String>, String> {
    let _ = DESKTOP_SERVICE;
    Err("此平台暂未接入 Desktop 凭据，请使用 gh auth login。".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_formats_do_not_return_refresh_or_malformed_tokens() {
        assert_eq!(desktop_token(b"gho_example"), Some("gho_example".into()));
        assert_eq!(desktop_token(br#"github-desktop-oauth:{"version":1,"credential":{"accessToken":"ghu_access","refreshToken":"ghr_refresh"}}"#),Some("ghu_access".into()));
        for bytes in [
            b"".as_slice(),
            b"bad\nheader",
            b"\xff",
            br#"github-desktop-oauth:{"version":2,"credential":{"accessToken":"x"}}"#,
            br#"github-desktop-oauth:{"version":1,"credential":null}"#,
        ] {
            assert!(desktop_token(bytes).is_none());
        }
    }

    #[tokio::test]
    async fn rejected_or_missing_cli_falls_back_to_desktop() {
        for missing in [false, true] {
            let (credential, _) = select(
                |source| async move {
                    match source {
                        Source::Cli if missing => Err("未登录".into()),
                        Source::Cli => Ok(vec!["cli_invalid".into()]),
                        Source::Desktop => Ok(vec!["desktop_valid".into()]),
                    }
                },
                |token| async move {
                    if token == "desktop_valid" {
                        Ok(serde_json::json!({"tag_name":"v0.3.1"}))
                    } else {
                        Err(ValidationError::Rejected("无权限"))
                    }
                },
            )
            .await
            .unwrap_or_else(|_| panic!("expected Desktop fallback"));
            assert_eq!(credential.source.label(), "GitHub Desktop");
        }
    }

    #[tokio::test]
    async fn working_cli_does_not_touch_desktop_store() {
        let (credential, _) = select(
            |source| async move {
                assert!(matches!(source, Source::Cli));
                Ok(vec!["cli_valid".into()])
            },
            |_| async { Ok(serde_json::json!({})) },
        )
        .await
        .unwrap_or_else(|_| panic!("expected CLI success"));
        assert_eq!(credential.source.label(), "GitHub CLI");
    }

    #[tokio::test]
    async fn failed_providers_and_network_have_safe_distinct_errors() {
        let error = select(
            |source| async move { Err(format!("{}未登录或读取被拒绝", source.label())) },
            |_| async { Ok(serde_json::json!({})) },
        )
        .await
        .err()
        .unwrap();
        assert!(error.contains("GitHub CLI") && error.contains("GitHub Desktop"));
        let error = select(
            |_| async { Ok(vec!["secret_test_token".into()]) },
            |_| async { Err(ValidationError::Fatal("网络失败".into())) },
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error, "网络失败");
        assert!(!error.contains("secret_test_token"));
    }

    #[tokio::test]
    #[ignore = "requires the user's GitHub Desktop login and live private-repository access"]
    async fn desktop_live_release_read_without_cli() {
        let tokens = desktop_tokens(false).unwrap_or_else(|error| panic!("{error}"));
        assert!(!tokens.is_empty(), "Desktop login was not readable");
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("Flow2API-Desktop-auth-verification")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();
        let mut authorized = false;
        for token in tokens {
            let response = client
                .get(RELEASE_URL)
                .bearer_auth(token)
                .send()
                .await
                .unwrap_or_else(|_| panic!("GitHub connection failed"));
            if response.status().as_u16() == 200 {
                let release: Value = response.json().await.unwrap();
                assert!(release["tag_name"]
                    .as_str()
                    .is_some_and(|tag| tag.starts_with('v')));
                authorized = true;
                println!("GitHub Desktop credential: private release HTTP 200; CLI not used");
                break;
            }
        }
        assert!(authorized, "Desktop login cannot read the private release");
    }
}
