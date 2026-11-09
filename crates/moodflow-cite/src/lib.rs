//! Offline bibliography interchange and citation validation. Network consent belongs to the host.
use anyhow::{Context, bail, ensure};
use hayagriva::{BibliographyDriver, BibliographyRequest, CitationItem, CitationRequest};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const MAX_IMPORT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_IMPORT_ENTRIES: usize = 2000;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Reference {
    pub title: String,
    pub kind: String,
    pub authors: String,
    pub year: String,
    pub container: String,
    pub publisher: String,
    pub volume: String,
    pub issue: String,
    pub pages: String,
    pub doi: String,
    pub url: String,
    pub isbn: String,
    /// Preserve fields and structured authors not represented in the simple editor.
    pub csl: Value,
}

impl Reference {
    pub fn missing(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for (name, value) in [
            ("title", &self.title),
            ("authors", &self.authors),
            ("year", &self.year),
        ] {
            if value.trim().is_empty() {
                out.push(name);
            }
        }
        if matches!(self.kind.as_str(), "journalArticle" | "conferencePaper") {
            if self.container.trim().is_empty() {
                out.push("container");
            }
        } else if self.publisher.trim().is_empty() {
            out.push("publisher");
        }
        out
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            ["book", "journalArticle", "conferencePaper", "report"].contains(&self.kind.as_str()),
            "unsupported_reference_type"
        );
        for value in [
            &self.title,
            &self.authors,
            &self.year,
            &self.container,
            &self.publisher,
            &self.volume,
            &self.issue,
            &self.pages,
            &self.doi,
            &self.url,
            &self.isbn,
        ] {
            ensure!(
                value.len() <= 8192
                    && !value
                        .chars()
                        .any(|c| c.is_control() && c != '\n' && c != '\t'),
                "invalid_reference"
            );
        }
        ensure!(
            serde_json::to_vec(&self.csl)?.len() <= 128 * 1024,
            "reference_limit"
        );
        if !self.year.trim().is_empty() {
            ensure!(
                self.year.trim().len() == 4 && self.year.trim().bytes().all(|b| b.is_ascii_digit()),
                "invalid_year"
            );
        }
        if !self.doi.trim().is_empty() {
            normalize_doi(&self.doi)?;
        }
        Ok(())
    }

    pub fn to_csl(&self) -> anyhow::Result<Value> {
        self.validate()?;
        let mut out = self.csl.as_object().cloned().unwrap_or_default();
        out.entry("id").or_insert(json!("moodflow-reference"));
        out.insert(
            "type".into(),
            json!(match self.kind.as_str() {
                "journalArticle" => "article-journal",
                "conferencePaper" => "paper-conference",
                "report" => "report",
                _ => "book",
            }),
        );
        let previous_names = names(&self.csl);
        if self.authors.trim() != previous_names {
            let author: Vec<Value> = self
                .authors
                .split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| match s.split_once(',') {
                    Some((family, given)) => {
                        json!({"family": family.trim(), "given": given.trim()})
                    }
                    None => json!({"literal":s}),
                })
                .collect();
            out.insert("author".into(), json!(author));
        }
        for (key, value) in [
            ("title", &self.title),
            ("container-title", &self.container),
            ("publisher", &self.publisher),
            ("volume", &self.volume),
            ("issue", &self.issue),
            ("page", &self.pages),
            ("URL", &self.url),
            ("ISBN", &self.isbn),
        ] {
            if value.trim().is_empty() {
                out.remove(key);
            } else {
                out.insert(key.into(), json!(value.trim()));
            }
        }
        if self.doi.trim().is_empty() {
            out.remove("DOI");
        } else {
            out.insert("DOI".into(), json!(normalize_doi(&self.doi)?));
        }
        let old_year = year(&self.csl);
        if self.year.trim() != old_year {
            if self.year.trim().is_empty() {
                out.remove("issued");
            } else {
                out.insert(
                    "issued".into(),
                    json!({"date-parts":[[self.year.trim().parse::<u32>()?]]}),
                );
            }
        }
        // Hayagriva currently panics on date ranges. A single bibliography entry
        // uses the range's first date; the original CSL remains in `self.csl`.
        for key in [
            "issued",
            "accessed",
            "event-date",
            "original-date",
            "submitted",
        ] {
            if let Some(parts) = out
                .get_mut(key)
                .and_then(|v| v.get_mut("date-parts"))
                .and_then(Value::as_array_mut)
            {
                parts.truncate(1);
            }
        }
        Ok(Value::Object(out))
    }

    pub fn from_csl(value: Value) -> anyhow::Result<Self> {
        ensure!(value.is_object(), "invalid_csl");
        let kind = match value.get("type").and_then(Value::as_str).unwrap_or("book") {
            "article-journal" | "journal-article" => "journalArticle",
            "paper-conference" | "proceedings-article" => "conferencePaper",
            "report" | "report-component" => "report",
            "book" | "chapter" | "monograph" | "edited-book" | "book-chapter" => "book",
            _ => bail!("unsupported_reference_type"),
        };
        let out = Self {
            kind: kind.into(),
            title: scalar(&value, "title"),
            authors: names(&value),
            year: year(&value),
            container: scalar(&value, "container-title"),
            publisher: scalar(&value, "publisher"),
            volume: scalar(&value, "volume"),
            issue: scalar(&value, "issue"),
            pages: scalar(&value, "page"),
            doi: scalar(&value, "DOI"),
            url: scalar(&value, "URL"),
            isbn: scalar(&value, "ISBN"),
            csl: value,
        };
        out.validate()?;
        Ok(out)
    }
}

fn scalar(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Array(v)) => v.first().and_then(Value::as_str).unwrap_or("").to_owned(),
        _ => String::new(),
    }
}
fn year(value: &Value) -> String {
    value
        .pointer("/issued/date-parts/0/0")
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| v.to_string())
        })
        .unwrap_or_default()
}
fn names(value: &Value) -> String {
    value
        .get("author")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|n| {
            if let Some(s) = n.get("literal").and_then(Value::as_str) {
                s.to_owned()
            } else {
                let family = scalar(n, "family");
                let given = scalar(n, "given");
                if given.is_empty() {
                    family
                } else {
                    format!("{family}, {given}")
                }
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

pub fn normalize_doi(input: &str) -> anyhow::Result<String> {
    let mut doi = input.trim();
    for prefix in [
        "https://doi.org/",
        "http://doi.org/",
        "https://dx.doi.org/",
        "doi:",
    ] {
        if doi
            .get(..prefix.len())
            .is_some_and(|s| s.eq_ignore_ascii_case(prefix))
        {
            doi = &doi[prefix.len()..];
            break;
        }
    }
    let (prefix, suffix) = doi.split_once('/').context("invalid_doi")?;
    let registrant = prefix.strip_prefix("10.").context("invalid_doi")?;
    ensure!(
        (4..=9).contains(&registrant.len())
            && registrant.bytes().all(|b| b.is_ascii_digit())
            && !suffix.is_empty()
            && doi.len() <= 256
            && doi
                .bytes()
                .all(|b| (b'!'..=b'~').contains(&b) && b != b'\\'),
        "invalid_doi"
    );
    Ok(doi.to_owned())
}

pub fn format(reference: &Reference, style: &str) -> anyhow::Result<String> {
    reference.validate()?;
    ensure!(reference.missing().is_empty(), "reference_missing_fields");
    if style == "gbt-compatible" {
        return Ok(gbt_compatible(reference));
    }
    ensure!(
        [
            "apa",
            "modern-language-association",
            "chicago-author-date",
            "vancouver",
            "ieee",
            "gb-7714-2015-numeric"
        ]
        .contains(&style),
        "unsupported_style"
    );
    let style = hayagriva::archive::ArchivedStyle::by_name(style)
        .context("unsupported_style")?
        .get();
    let hayagriva::citationberg::Style::Independent(mut style) = style else {
        bail!("unsupported_style");
    };
    for item in &mut style.macros {
        materialize_implicit_names(&mut item.children);
    }
    materialize_implicit_names(&mut style.citation.layout.elements);
    if let Some(bibliography) = &mut style.bibliography {
        materialize_implicit_names(&mut bibliography.layout.elements);
    }
    let mut render_data = reference.to_csl()?;
    if let Some(fields) = render_data.as_object_mut() {
        fields.retain(|key, value| {
            use hayagriva::citationberg::taxonomy::Variable;
            let variable = serde_json::from_value::<Variable>(json!(key)).ok();
            if variable.is_none() && key != "id" && key != "type" {
                return false;
            }
            // Some DOI providers append non-CSL objects or return a list for a
            // scalar such as ISSN. Keep the original exchange data untouched.
            if matches!(variable, Some(Variable::Standard(_))) {
                if value.is_array() {
                    *value = value
                        .as_array()
                        .and_then(|a| a.first())
                        .cloned()
                        .unwrap_or(Value::Null);
                }
                if value.is_object() {
                    *value = value
                        .get("URL")
                        .or_else(|| value.get("literal"))
                        .cloned()
                        .unwrap_or(Value::Null);
                }
                if !value.is_string() && !value.is_number() {
                    return false;
                }
            }
            true
        });
    }
    let item = hayagriva::citationberg::json::Item(
        render_data
            .as_object()
            .context("invalid_csl")?
            .iter()
            .map(|(key, value)| {
                let parsed = serde_json::from_value(value.clone())
                    .with_context(|| format!("invalid_csl_field:{key}"))?;
                Ok((key.clone(), parsed))
            })
            .collect::<anyhow::Result<_>>()?,
    );
    let locales = hayagriva::archive::locales();
    let mut driver = BibliographyDriver::new();
    driver.citation(CitationRequest::from_items(
        vec![CitationItem::with_entry(&item)],
        &style,
        &locales,
    ));
    let rendered = driver.finish(BibliographyRequest::new(&style, None, &locales));
    let bibliography = rendered.bibliography.context("style_has_no_bibliography")?;
    let entry = bibliography.items.first().context("empty_bibliography")?;
    let prefix = entry
        .first_field
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default();
    Ok(format!(
        "{prefix}{}{}",
        if prefix.is_empty() { "" } else { " " },
        entry.content
    ))
}

// Hayagriva 0.10.1's visibility check treats cs:names containing only a label /
// substitute as empty, although rendering uses the implicit default cs:name.
// Materialize that default in the loaded style, retaining inherited options.
fn materialize_implicit_names(elements: &mut [hayagriva::citationberg::LayoutRenderingElement]) {
    use hayagriva::citationberg::{LayoutRenderingElement as Element, NamesChild};
    for element in elements {
        match element {
            Element::Names(names) => {
                if names.name().is_none() && !names.children.is_empty() {
                    names
                        .children
                        .insert(0, NamesChild::Name(Default::default()));
                }
                for child in &mut names.children {
                    if let NamesChild::Substitute(substitute) = child {
                        materialize_implicit_names(&mut substitute.children);
                    }
                }
            }
            Element::Group(group) => materialize_implicit_names(&mut group.children),
            Element::Choose(choose) => {
                materialize_implicit_names(&mut choose.if_.children);
                for branch in &mut choose.else_if {
                    materialize_implicit_names(&mut branch.children);
                }
                if let Some(branch) = &mut choose.otherwise {
                    materialize_implicit_names(&mut branch.children);
                }
            }
            _ => {}
        }
    }
}

// Adapted from CapsuleReference.swift (XSIME, Apache-2.0).
// Copyright XSIME contributors. Source and full license are distributed in licenses/.
// https://github.com/scholay-xsime/xsime/blob/main/Sources/RimeBuffer/CapsuleReference.swift
fn gbt_compatible(r: &Reference) -> String {
    let authors = r
        .authors
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    let source = if matches!(r.kind.as_str(), "book" | "report") {
        &r.publisher
    } else {
        &r.container
    };
    let detail = format!(
        "{}{}{}",
        r.volume,
        if r.issue.is_empty() {
            String::new()
        } else {
            format!("({})", r.issue)
        },
        if r.pages.is_empty() {
            String::new()
        } else {
            format!(": {}", r.pages)
        }
    );
    let marker = match r.kind.as_str() {
        "journalArticle" => "J",
        "conferencePaper" => "C",
        "report" => "R",
        _ => "M",
    };
    let locator = if r.doi.is_empty() {
        r.url.clone()
    } else {
        format!(
            "https://doi.org/{}",
            normalize_doi(&r.doi).unwrap_or_default()
        )
    };
    format!(
        "{authors}. {}[{marker}]. {source}, {}{}.{}",
        r.title.trim(),
        r.year.trim(),
        if detail.is_empty() {
            String::new()
        } else {
            format!(", {detail}")
        },
        if locator.is_empty() {
            String::new()
        } else {
            format!(" {locator}")
        }
    )
}

pub fn import_csl(text: &str) -> anyhow::Result<Vec<Reference>> {
    ensure!(text.len() <= MAX_IMPORT_BYTES, "reference_import_limit");
    let value: Value = serde_json::from_str(text).context("invalid_csl")?;
    let items = match value {
        Value::Array(v) => v,
        v => vec![v],
    };
    ensure!(
        !items.is_empty() && items.len() <= MAX_IMPORT_ENTRIES,
        "reference_import_limit"
    );
    items.into_iter().map(Reference::from_csl).collect()
}

pub fn import_bibtex(text: &str) -> anyhow::Result<Vec<Reference>> {
    ensure!(text.len() <= MAX_IMPORT_BYTES, "reference_import_limit");
    let library =
        hayagriva::io::from_biblatex_str(text).map_err(|_| anyhow::anyhow!("invalid_bibtex"))?;
    ensure!(
        !library.is_empty() && library.len() <= MAX_IMPORT_ENTRIES,
        "reference_import_limit"
    );
    library
        .iter()
        .map(|entry| {
            let kind = match entry.entry_type() {
                hayagriva::types::EntryType::Article
                    if entry
                        .parents()
                        .iter()
                        .any(|p| *p.entry_type() == hayagriva::types::EntryType::Proceedings) =>
                {
                    "conferencePaper"
                }
                hayagriva::types::EntryType::Article => "journalArticle",
                hayagriva::types::EntryType::Anthos => "conferencePaper",
                hayagriva::types::EntryType::Report => "report",
                _ => "book",
            };
            let mut r = Reference {
                title: entry.title().map(ToString::to_string).unwrap_or_default(),
                kind: kind.into(),
                authors: entry
                    .authors()
                    .unwrap_or(&[])
                    .iter()
                    .map(|p| p.name_first(false, false))
                    .collect::<Vec<_>>()
                    .join("; "),
                year: entry
                    .date_any()
                    .map(|d| d.year.to_string())
                    .unwrap_or_default(),
                container: entry
                    .map_parents(|e| e.title().map(ToString::to_string))
                    .unwrap_or_default(),
                publisher: entry
                    .map(|e| {
                        e.publisher()
                            .and_then(|p| p.name())
                            .map(ToString::to_string)
                    })
                    .unwrap_or_default(),
                volume: entry
                    .map(|e| e.volume().map(ToString::to_string))
                    .unwrap_or_default(),
                issue: entry
                    .map(|e| e.issue().map(ToString::to_string))
                    .unwrap_or_default(),
                pages: entry
                    .page_range()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                doi: entry.doi().unwrap_or("").into(),
                isbn: entry.isbn().unwrap_or("").into(),
                url: entry
                    .url_any()
                    .map(|u| u.value.to_string())
                    .unwrap_or_default(),
                ..Default::default()
            };
            r.csl = r.to_csl()?;
            r.csl["id"] = json!(entry.key());
            Ok(r)
        })
        .collect()
}

pub fn export_bibtex(r: &Reference) -> anyhow::Result<String> {
    r.validate()?;
    let ty = match r.kind.as_str() {
        "journalArticle" => "article",
        "conferencePaper" => "inproceedings",
        "report" => "techreport",
        _ => "book",
    };
    let csl = r.to_csl()?;
    let authors = names(&csl)
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>()
        .join(" and ");
    let mut out = format!("@{ty}{{moodflow,\n");
    for (key, value) in [
        ("title", &r.title),
        ("author", &authors),
        ("year", &r.year),
        (
            if ty == "article" {
                "journal"
            } else {
                "booktitle"
            },
            &r.container,
        ),
        ("publisher", &r.publisher),
        ("volume", &r.volume),
        ("number", &r.issue),
        ("pages", &r.pages),
        ("doi", &r.doi),
        ("url", &r.url),
        ("isbn", &r.isbn),
    ] {
        if !value.is_empty() {
            out.push_str(&format!("  {key} = {{{}}},\n", escape_bibtex(value)));
        }
    }
    out.push_str("}\n");
    Ok(out)
}
fn escape_bibtex(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '{' | '}' | '%' | '&' | '#' | '_' | '$' => {
                out.push('\\');
                out.push(c);
            }
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            '\n' | '\r' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Validate every bracketed reference before joining it with an ordered source list.
pub fn cited_text(text: &str, references: &[String]) -> anyhow::Result<String> {
    ensure!(
        !references.is_empty() && references.len() <= 100 && text.len() <= 64 * 1024,
        "invalid_citations"
    );
    ensure!(
        references
            .iter()
            .all(|r| !r.trim().is_empty() && r.len() <= 16384),
        "invalid_citations"
    );
    let normalized = text
        .replace('【', "[")
        .replace('】', "]")
        .replace(['，', '、'], ",")
        .replace('–', "-");
    let mut rest = normalized.as_str();
    let mut count = 0;
    while let Some(start) = rest.find('[') {
        ensure!(!rest[..start].contains(']'), "invalid_citations");
        rest = &rest[start + 1..];
        let end = rest.find(']').context("invalid_citations")?;
        let content = &rest[..end];
        ensure!(!content.is_empty(), "invalid_citations");
        for token in content.split(',') {
            let nums = token
                .split('-')
                .map(|s| s.trim().parse::<usize>())
                .collect::<Result<Vec<_>, _>>()
                .context("invalid_citations")?;
            ensure!(
                (1..=2).contains(&nums.len())
                    && nums.iter().all(|n| (1..=references.len()).contains(n))
                    && (nums.len() == 1 || nums[0] <= nums[1]),
                "invalid_citations"
            );
            count += 1;
        }
        rest = &rest[end + 1..];
    }
    ensure!(!rest.contains(']') && count > 0, "missing_citations");
    let bibliography = references
        .iter()
        .enumerate()
        .map(|(i, r)| format!("[{}] {r}", i + 1))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!("{}\n\n{bibliography}", text.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn article() -> Reference {
        Reference {
            title: "样例研究".into(),
            kind: "journalArticle".into(),
            authors: "张三; 李四".into(),
            year: "2024".into(),
            container: "样例期刊".into(),
            volume: "12".into(),
            issue: "3".into(),
            pages: "1-9".into(),
            doi: "10.1234/example".into(),
            ..Default::default()
        }
    }
    #[test]
    fn xsime_default_matches_and_required_metadata_is_enforced() {
        let mut r = article();
        assert_eq!(
            format(&r, "gbt-compatible").unwrap(),
            "张三, 李四. 样例研究[J]. 样例期刊, 2024, 12(3): 1-9. https://doi.org/10.1234/example"
        );
        r.authors.clear();
        assert!(format(&r, "apa").is_err());
    }
    #[test]
    fn csl_fields_names_dates_and_formal_styles_survive() {
        let value = json!({"id":"a", "type":"article-journal", "title":"Research", "author":[{"family":"Smith","given":"Jane"}],
            "issued":{"date-parts":[[2024,5,2],[2024,6,2]]}, "container-title":"Journal", "custom":"retained",
            "indexed":{"date-parts":[[2026,7,1]]},"ISSN":["1234-5678"],"license":[{"URL":"https://example.invalid/license"}]});
        let r = Reference::from_csl(value).unwrap();
        assert_eq!(r.to_csl().unwrap()["author"][0]["given"], "Jane");
        assert_eq!(r.to_csl().unwrap()["custom"], "retained");
        for style in [
            "apa",
            "modern-language-association",
            "chicago-author-date",
            "vancouver",
            "ieee",
            "gb-7714-2015-numeric",
        ] {
            let output = format(&r, style).unwrap();
            assert!(output.to_lowercase().contains("smith"), "{style}: {output}");
            assert!(output.contains("Research") || output.contains("research"));
        }
    }
    #[test]
    fn bibtex_interchange_and_citation_fabrication_guard() {
        let r = article();
        let imported = import_bibtex(&export_bibtex(&r).unwrap()).unwrap();
        assert_eq!(imported[0].title, r.title);
        assert_eq!(imported[0].doi, r.doi);
        assert_eq!(imported[0].year, r.year);
        assert!(
            cited_text("Known [1].", &["Source".into()])
                .unwrap()
                .ends_with("[1] Source")
        );
        for text in [
            "Unknown [2].",
            "Zero [0].",
            "None",
            "Broken [1",
            "Fake [author].",
            "Range [1-3].",
            "Mixed [1] and 【99】.",
        ] {
            assert!(cited_text(text, &["Source".into()]).is_err());
        }
        assert_eq!(
            normalize_doi("https://doi.org/10.1234/a?b#c").unwrap(),
            "10.1234/a?b#c"
        );
        assert!(normalize_doi("https://other.invalid/a").is_err());
        assert!(normalize_doi("10.1234/a\nContent: injected").is_err());
    }
}
