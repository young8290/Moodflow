//! Single-purpose, consented DOI lookup. Only a DOI is put on the wire.
use futures::StreamExt;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
#[error("doi_lookup_failed")]
pub struct LookupError;

fn endpoint(doi: &str) -> Result<reqwest::Url, LookupError> {
    let mut url =
        reqwest::Url::parse("https://api.crossref.org/v1/works/").map_err(|_| LookupError)?;
    url.path_segments_mut()
        .map_err(|_| LookupError)?
        .pop_if_empty()
        .push(doi)
        .push("transform");
    Ok(url)
}

pub async fn lookup(input: &str) -> Result<moodflow_cite::Reference, LookupError> {
    let doi = moodflow_cite::normalize_doi(input).map_err(|_| LookupError)?;
    let url = endpoint(&doi)?;
    // Pin the service origin and disable redirects. Never follow a DOI to a publisher.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .user_agent("Moodflow/1.0 (user-initiated DOI metadata lookup)")
        .build()
        .map_err(|_| LookupError)?;
    let response = client
        .get(url)
        .header("Accept", "application/vnd.citationstyles.csl+json")
        .send()
        .await
        .map_err(|_| LookupError)?;
    if !response.status().is_success() || response.content_length().is_some_and(|n| n > 1024 * 1024)
    {
        return Err(LookupError);
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| LookupError)?;
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err(LookupError);
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| LookupError)?;
    let reference = moodflow_cite::Reference::from_csl(value).map_err(|_| LookupError)?;
    // Do not silently accept an unrelated response under the requested identifier.
    if moodflow_cite::normalize_doi(&reference.doi)
        .map_err(|_| LookupError)?
        .to_lowercase()
        != doi.to_lowercase()
    {
        return Err(LookupError);
    }
    Ok(reference)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doi_stays_a_single_path_segment_on_the_fixed_origin() {
        let url = endpoint("10.1234/a?b#c/../x").unwrap();
        assert_eq!(
            url.origin().ascii_serialization(),
            "https://api.crossref.org"
        );
        assert_eq!(
            url.path(),
            "/v1/works/10.1234%2Fa%3Fb%23c%2F..%2Fx/transform"
        );
        assert!(url.query().is_none());
        assert!(url.fragment().is_none());
    }

    #[tokio::test]
    async fn invalid_identifiers_fail_before_any_network_request() {
        for invalid in [
            "",
            "https://other.invalid/secret",
            "10.1234/has space",
            "10.1234/a\r\nHeader: value",
        ] {
            assert!(lookup(invalid).await.is_err());
        }
    }

    #[tokio::test]
    #[ignore = "requires an explicit online check against the public Crossref example"]
    async fn public_crossref_metadata_smoke() {
        let reference = lookup("10.1038/nphys1170").await.unwrap();
        assert_eq!(reference.doi.to_lowercase(), "10.1038/nphys1170");
        assert!(!reference.title.trim().is_empty());
        assert!(!reference.authors.trim().is_empty());
        assert!(moodflow_cite::format(&reference, "apa").is_ok());
    }
}
