//! Per-device values that must stay out of git (personal emails and the like).
//! Read order: process environment first, then `<app data>/local.env`.

use std::path::Path;

pub const PREFERRED_EMAILS: &str = "FLOW2API_PREFERRED_EMAILS";
const LOCAL_ENV_FILE: &str = "local.env";

/// Quick-fill Google accounts. Empty when nothing is configured on this device.
pub fn preferred_emails(data_dir: Option<&Path>) -> Vec<String> {
    let from_env = std::env::var(PREFERRED_EMAILS)
        .ok()
        .filter(|value| !value.trim().is_empty());
    let raw = from_env.or_else(|| {
        let text = std::fs::read_to_string(data_dir?.join(LOCAL_ENV_FILE)).ok()?;
        lookup(&text, PREFERRED_EMAILS)
    });
    raw.map(|value| parse_emails(&value)).unwrap_or_default()
}

/// Reads `KEY=value` from a dotenv-style file; `#` lines are comments.
fn lookup(text: &str, key: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .map(|line| line.strip_prefix("export ").unwrap_or(line))
        .find_map(|line| {
            let (name, value) = line.split_once('=')?;
            (name.trim() == key).then(|| value.trim().trim_matches(['"', '\'']).to_string())
        })
}

fn parse_emails(value: &str) -> Vec<String> {
    let mut emails: Vec<String> = Vec::new();
    for email in value.split([',', ';', ' ']).map(str::trim) {
        if email.contains('@') && !emails.iter().any(|e| e.eq_ignore_ascii_case(email)) {
            emails.push(email.to_string());
        }
    }
    emails
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_key_from_dotenv_text() {
        let text = "# local only\nOTHER=1\nexport FLOW2API_PREFERRED_EMAILS=\"a@example.com, b@example.com\"\n";
        assert_eq!(
            lookup(text, PREFERRED_EMAILS).as_deref(),
            Some("a@example.com, b@example.com")
        );
        assert!(lookup(
            "# FLOW2API_PREFERRED_EMAILS=x@example.com",
            PREFERRED_EMAILS
        )
        .is_none());
    }

    #[test]
    fn parses_and_dedupes_emails() {
        assert_eq!(
            parse_emails("a@example.com;B@example.com, b@example.com  not-an-email"),
            vec!["a@example.com", "B@example.com"]
        );
        assert!(parse_emails("").is_empty());
    }

    #[test]
    fn falls_back_to_local_env_file() {
        if std::env::var_os(PREFERRED_EMAILS).is_some() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("flow2api-local-env-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(LOCAL_ENV_FILE),
            "FLOW2API_PREFERRED_EMAILS=c@example.com\n",
        )
        .unwrap();
        assert_eq!(preferred_emails(Some(&dir)), vec!["c@example.com"]);
        assert!(preferred_emails(None).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
