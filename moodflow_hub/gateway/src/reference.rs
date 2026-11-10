//! Explicit Crossref search; source identities and abstracts are obtained from the official API.
use futures::StreamExt;
use moodflow_cite::Reference;
use serde::Serialize;
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};

#[derive(Clone, Serialize)]
pub struct Source {
    pub reference: Reference,
    pub abstract_text: String,
}
#[derive(Debug, thiserror::Error)]
#[error("reference_search_failed")]
pub struct SearchError;

fn endpoint(query: &str) -> Result<reqwest::Url, SearchError> {
    if query.trim().is_empty()
        || query.encode_utf16().count() > 512
        || query.chars().any(char::is_control)
    {
        return Err(SearchError);
    }
    let mut url = reqwest::Url::parse("https://api.crossref.org/works").map_err(|_| SearchError)?;
    url.query_pairs_mut()
        .append_pair("query.bibliographic", query)
        .append_pair("filter", "has-abstract:true")
        .append_pair("rows", "12");
    Ok(url)
}
fn plain_abstract(input: &str) -> String {
    let mut tag = false;
    input
        .chars()
        .filter(|c| match c {
            '<' => {
                tag = true;
                false
            }
            '>' => {
                tag = false;
                false
            }
            _ => !tag,
        })
        .take(6000)
        .collect::<String>()
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}
fn parse(value: Value) -> Result<Vec<Source>, SearchError> {
    let items = value
        .pointer("/message/items")
        .and_then(Value::as_array)
        .ok_or(SearchError)?;
    if items.len() > 12 {
        return Err(SearchError);
    }
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for item in items {
        let Some(abstract_text) = item["abstract"]
            .as_str()
            .filter(|t| t.len() <= 64 * 1024)
            .map(plain_abstract)
            .filter(|t| !t.trim().is_empty())
        else {
            continue;
        };
        let Some(doi) = item["DOI"]
            .as_str()
            .and_then(|s| moodflow_cite::normalize_doi(s).ok())
        else {
            continue;
        };
        if !seen.insert(doi.to_lowercase()) {
            continue;
        }
        let Some(title) = item["title"].get(0).and_then(Value::as_str) else {
            continue;
        };
        let kind = match item["type"].as_str() {
            Some("journal-article") => "article-journal",
            Some("proceedings-article") => "paper-conference",
            Some("book" | "monograph") => "book",
            _ => continue,
        };
        let Some(date) = ["published", "published-print", "published-online", "issued"]
            .iter()
            .find_map(|key| item.get(*key).filter(|v| v.get("date-parts").is_some()))
        else {
            continue;
        };
        let csl = json!({"id":doi,"DOI":doi,"type":kind,"title":title,"author":item["author"],"issued":date,"container-title":item["container-title"].get(0).cloned().unwrap_or(Value::Null),"publisher":item["publisher"],"volume":item["volume"],"issue":item["issue"],"page":item["page"],"URL":format!("https://doi.org/{doi}")});
        let Ok(reference) = Reference::from_csl(csl) else {
            continue;
        };
        if reference.missing().is_empty() {
            out.push(Source {
                reference,
                abstract_text,
            });
            if out.len() == 4 {
                break;
            }
        }
    }
    Ok(out)
}
pub async fn search(query: &str) -> Result<Vec<Source>, SearchError> {
    let url = endpoint(query)?;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .user_agent("Moodflow/1.0 (user-initiated bibliography search)")
        .build()
        .map_err(|_| SearchError)?;
    let response = client.get(url).send().await.map_err(|_| SearchError)?;
    if !response.status().is_success() || response.content_length().is_some_and(|n| n > 1024 * 1024)
    {
        return Err(SearchError);
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| SearchError)?;
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err(SearchError);
        }
        bytes.extend_from_slice(&chunk);
    }
    parse(serde_json::from_slice(&bytes).map_err(|_| SearchError)?)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn official_origin_query_encoding_and_required_metadata() {
        let url = endpoint("alpha & beta? x=secret").unwrap();
        assert_eq!(
            url.origin().ascii_serialization(),
            "https://api.crossref.org"
        );
        assert_eq!(
            url.query_pairs()
                .find(|(k, _)| k == "query.bibliographic")
                .unwrap()
                .1,
            "alpha & beta? x=secret"
        );
        assert!(endpoint(&"x".repeat(513)).is_err());
        let item = json!({"DOI":"10.1234/real","title":["A real title"],"author":[{"family":"Doe","given":"Jane"}],"published":{"date-parts":[[2025]]},"type":"journal-article","container-title":["Journal"],"abstract":"<jats:p>Evidence &amp; context.</jats:p>"});
        let sources =
            parse(json!({"message":{"items":[item.clone(),item,{"DOI":"10.1234/no-abstract"}]}}))
                .unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].abstract_text, "Evidence & context.");
        assert_eq!(sources[0].reference.doi, "10.1234/real");
    }
    #[tokio::test]
    #[ignore = "explicit online check against Crossref"]
    async fn official_search_smoke() {
        let sources = search("quantum entanglement").await.unwrap();
        assert!(!sources.is_empty());
        for source in sources {
            assert!(!source.abstract_text.is_empty());
            assert!(moodflow_cite::format(&source.reference, "apa").is_ok());
        }
    }
}
