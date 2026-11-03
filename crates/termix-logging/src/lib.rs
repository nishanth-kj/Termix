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
    const KEYS: &[&str] = &[
        "password=",
        "passwd=",
        "pwd=",
        "passphrase=",
        "secret=",
        "private_key=",
        "token=",
        "api_key=",
        "apikey=",
        "access_key=",
        "--password ",
        "--passphrase ",
        "--secret ",
        "--token ",
        "--api-key ",
        "sshpass -p ",
        "authorization: bearer ",
        "authorization: basic ",
    ];
    let mut result = String::with_capacity(input.len());
    let lower = input.to_ascii_lowercase();
    let mut i = 0;
    while i < input.len() {
        if let Some(key) = KEYS.iter().find(|key| lower[i..].starts_with(**key)) {
            result.push_str(&input[i..i + key.len()]);
            result.push_str("[redacted]");
            i += key.len();
            i += secret_len(&input[i..]);
            continue;
        }
        if input[..i].ends_with("://") {
            if let Some(length) = url_credentials_len(&input[i..]) {
                result.push_str("[redacted]");
                i += length;
                continue;
            }
        }
        let ch = input[i..].chars().next().expect("i is on a char boundary");
        result.push(ch);
        i += ch.len_utf8();
    }
    redact_attached_password_flag(&result)
}

/// Length of a value, including its quotes when it is quoted.
fn secret_len(value: &str) -> usize {
    let mut chars = value.char_indices();
    match chars.next() {
        Some((_, quote @ ('"' | '\''))) => chars
            .find(|(_, ch)| *ch == quote)
            .map_or(value.len(), |(index, ch)| index + ch.len_utf8()),
        _ => value.find(char::is_whitespace).unwrap_or(value.len()),
    }
}

/// Length of `user:password` in `scheme://user:password@host`.
fn url_credentials_len(rest: &str) -> Option<usize> {
    let authority = &rest[..rest.find(['/', ' ', '\t', '\n']).unwrap_or(rest.len())];
    let at = authority.rfind('@')?;
    authority[..at].contains(':').then_some(at)
}

/// `mysql -psecret` attaches the password to the flag.
fn redact_attached_password_flag(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    if !lower.contains("mysql") && !lower.contains("mariadb") {
        return input.to_string();
    }
    input
        .split(' ')
        .map(|word| match word.strip_prefix("-p") {
            Some(value) if !value.is_empty() => "-p[redacted]".to_string(),
            _ => word.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
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

    #[test]
    fn redacts_every_common_way_to_pass_a_secret() {
        for raw in [
            "deploy --password hunter2 --verbose",
            "deploy --password=hunter2",
            "deploy --token hunter2",
            "curl -H 'Authorization: Bearer hunter2' example.com",
            "PGPASSWORD=hunter2 psql",
            "login password=\"hunter2 hunter2\"",
            "API_KEY=hunter2 ./run",
            "sshpass -p hunter2 ssh box",
            "mysql -uroot -phunter2 db",
            "git clone https://me:hunter2@example.com/repo.git",
        ] {
            let cleaned = redact(raw);
            assert!(!cleaned.contains("hunter2"), "{raw} -> {cleaned}");
        }
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        let raw = "uname -a && curl https://example.com/página -p 1";
        assert_eq!(redact(raw), raw);
    }
}
