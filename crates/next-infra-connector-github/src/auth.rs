use crate::GitHubError;
use next_infra_core::SecretValue;
use reqwest::header::HeaderValue;

pub(crate) fn authorization_header(secret: &SecretValue) -> Result<HeaderValue, GitHubError> {
    let token = std::str::from_utf8(secret.expose())
        .map_err(|_| GitHubError::authentication("GitHub token is not valid UTF-8"))?;
    if token.is_empty()
        || token.len() > 4096
        || token
            .bytes()
            .any(|byte| !byte.is_ascii_graphic() || byte == b' ')
    {
        return Err(GitHubError::authentication(
            "GitHub token is empty or contains invalid characters",
        ));
    }

    let mut header = HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|_| GitHubError::authentication("GitHub token cannot form an HTTP header"))?;
    header.set_sensitive(true);
    Ok(header)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorization_is_sensitive_and_rejects_unsafe_tokens() {
        let valid = SecretValue::new("github-token-sentinel");
        let header = authorization_header(&valid).unwrap();
        assert!(header.is_sensitive());
        assert!(!format!("{header:?}").contains("github-token-sentinel"));

        for invalid in [Vec::new(), b"line\nbreak".to_vec(), vec![0xff, 0xfe]] {
            assert!(authorization_header(&SecretValue::new(invalid)).is_err());
        }
    }
}
