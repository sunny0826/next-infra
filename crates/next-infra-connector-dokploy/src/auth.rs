use next_infra_core::SecretValue;
use reqwest::header::HeaderValue;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DokployAuthError;

impl std::fmt::Display for DokployAuthError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Dokploy credential is invalid")
    }
}

impl std::error::Error for DokployAuthError {}

pub fn authorization_header(secret: &SecretValue) -> Result<HeaderValue, DokployAuthError> {
    let token = std::str::from_utf8(secret.expose()).map_err(|_| DokployAuthError)?;
    if token.is_empty()
        || token.len() > 4096
        || token
            .bytes()
            .any(|byte| !byte.is_ascii_graphic() || byte == b' ')
    {
        return Err(DokployAuthError);
    }
    let mut header =
        HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| DokployAuthError)?;
    header.set_sensitive(true);
    Ok(header)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_never_appears_in_header_debug_output() {
        let header = authorization_header(&SecretValue::new("dokploy-token-sentinel")).unwrap();
        assert!(header.is_sensitive());
        assert!(!format!("{header:?}").contains("dokploy-token-sentinel"));
        assert!(authorization_header(&SecretValue::new("line\nbreak")).is_err());
    }
}
