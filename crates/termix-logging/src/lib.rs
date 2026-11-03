//! Logging helpers. Secrets must be redacted before they reach a log line.

use std::sync::Once;

static INIT: Once = Once::new();

pub fn init() {
    INIT.call_once(|| {
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
        let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
    });
}

/// Removes PEM blocks and common password assignments from text that might be logged.
pub fn redact(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    const BEGIN: &str = "-----BEGIN ";
    const END: &str = "-----END ";
    while let Some(start) = rest.find(BEGIN) {
        out.push_str(&rest[..start]);
        out.push_str("[redacted private key]");
        let after = &rest[start + BEGIN.len()..];
        if let Some(end_rel) = after.find(END) {
            let after_end = &after[end_rel + END.len()..];
            if let Some(nl) = after_end.find('\n') {
                rest = &after_end[nl + 1..];
            } else {
                rest = "";
            }
        } else {
            rest = "";
        }
    }
    out.push_str(rest);
    redact_assignments(&out)
}

fn redact_assignments(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let lower = input.to_ascii_lowercase();
    let bytes = input.as_bytes();
    let mut i = 0;
    let keys = ["password=", "passphrase=", "secret=", "private_key="];
    while i < bytes.len() {
        let mut matched = false;
        for key in keys {
            if lower[i..].starts_with(key) {
                result.push_str(&input[i..i + key.len()]);
                result.push_str("[redacted]");
                i += key.len();
                while i < bytes.len() && bytes[i] != b' ' && bytes[i] != b'\n' && bytes[i] != b'\t'
                {
                    i += 1;
                }
                matched = true;
                break;
            }
        }
        if !matched {
            result.push(input[i..].chars().next().unwrap());
            i += input[i..].chars().next().unwrap().len_utf8();
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_password_and_pem() {
        let raw = "login password=hunter2\n-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----\n";
        let cleaned = redact(raw);
        assert!(!cleaned.contains("hunter2"));
        assert!(!cleaned.contains("abc"));
        assert!(cleaned.contains("[redacted]"));
        assert!(cleaned.contains("[redacted private key]"));
    }
}
