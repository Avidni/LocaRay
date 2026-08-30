use regex::Regex;
use url::Url;

const MAX_ACCUMULATOR_BYTES: usize = 16 * 1024;

#[derive(Debug)]
pub struct PublicUrlExtractor {
    accumulator: String,
    ansi: Regex,
    candidates: Regex,
    accepted: bool,
}

impl PublicUrlExtractor {
    pub fn new() -> Result<Self, regex::Error> {
        Ok(Self {
            accumulator: String::new(),
            ansi: Regex::new(r"\x1B(?:\[[0-?]*[ -/]*[@-~]|\][^\x07]*(?:\x07|\x1B\\))")?,
            candidates: Regex::new(r#"https://[^\s<>\"']+"#)?,
            accepted: false,
        })
    }

    pub fn push(&mut self, chunk: &[u8]) -> Option<String> {
        if self.accepted {
            return None;
        }

        self.accumulator.push_str(&String::from_utf8_lossy(chunk));
        if self.accumulator.len() > MAX_ACCUMULATOR_BYTES {
            let keep_from = self.accumulator.len() - MAX_ACCUMULATOR_BYTES;
            self.accumulator =
                String::from_utf8_lossy(&self.accumulator.as_bytes()[keep_from..]).into_owned();
        }

        let clean = self.ansi.replace_all(&self.accumulator, "");
        for candidate in self.candidates.find_iter(&clean) {
            let trimmed = candidate
                .as_str()
                .trim_end_matches(['.', ',', ';', ':', ')', ']', '}']);
            if let Some(url) = validate_public_url(trimmed) {
                self.accepted = true;
                return Some(url);
            }
        }
        None
    }
}

pub fn validate_public_url(candidate: &str) -> Option<String> {
    let mut url = Url::parse(candidate).ok()?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }

    let host = url.host_str()?.to_ascii_lowercase();
    let label = host.strip_suffix(".trycloudflare.com")?;
    if label.is_empty()
        || label.len() > 63
        || label.contains('.')
        || !label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        || !label.as_bytes().first()?.is_ascii_alphanumeric()
        || !label.as_bytes().last()?.is_ascii_alphanumeric()
    {
        return None;
    }

    url.set_fragment(None);
    url.set_query(None);
    url.set_path("/");
    Some(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::{validate_public_url, PublicUrlExtractor};

    #[test]
    fn extracts_a_split_ansi_formatted_url() {
        let mut extractor = PublicUrlExtractor::new().expect("regexes should compile");
        assert!(extractor.push(b"\x1b[32mhttps://bright-").is_none());
        assert_eq!(
            extractor.push(b"field.trycloudflare.com\x1b[0m\n"),
            Some("https://bright-field.trycloudflare.com/".to_owned())
        );
    }

    #[test]
    fn emits_only_once() {
        let mut extractor = PublicUrlExtractor::new().expect("regexes should compile");
        assert!(extractor.push(b"https://first.trycloudflare.com").is_some());
        assert!(extractor
            .push(b"https://second.trycloudflare.com")
            .is_none());
    }

    #[test]
    fn rejects_malicious_near_matches_and_credentials() {
        for candidate in [
            "https://trycloudflare.com.attacker.example",
            "https://user@safe.trycloudflare.com",
            "https://safe.trycloudflare.com:444",
            "http://safe.trycloudflare.com",
            "https://trycloudflare.com",
            "https://nested.safe.trycloudflare.com",
            "https://-unsafe.trycloudflare.com",
            "https://unsafe-.trycloudflare.com",
            "https://unsafe_name.trycloudflare.com",
        ] {
            assert_eq!(validate_public_url(candidate), None, "{candidate}");
        }
    }

    #[test]
    fn normalizes_a_trusted_origin_to_its_root() {
        assert_eq!(
            validate_public_url("https://safe.trycloudflare.com/path?token=secret#section"),
            Some("https://safe.trycloudflare.com/".to_owned())
        );
    }
}
