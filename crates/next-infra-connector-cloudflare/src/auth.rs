use next_infra_core::SecretValue;
use reqwest::header::HeaderValue;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloudflareAuthError;

impl std::fmt::Display for CloudflareAuthError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Cloudflare API token is invalid")
    }
}

impl std::error::Error for CloudflareAuthError {}

pub fn authorization_header(secret: &SecretValue) -> Result<HeaderValue, CloudflareAuthError> {
    let token = std::str::from_utf8(secret.expose()).map_err(|_| CloudflareAuthError)?;
    if token.is_empty()
        || token.len() > 4096
        || token
            .bytes()
            .any(|byte| !byte.is_ascii_graphic() || byte == b' ')
    {
        return Err(CloudflareAuthError);
    }
    let mut header =
        HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| CloudflareAuthError)?;
    header.set_sensitive(true);
    Ok(header)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_header_is_sensitive_and_non_printing() {
        let header = authorization_header(&SecretValue::new("cloudflare-token-sentinel")).unwrap();
        assert!(header.is_sensitive());
        assert!(!format!("{header:?}").contains("cloudflare-token-sentinel"));
        assert!(authorization_header(&SecretValue::new("invalid token")).is_err());
    }
}
