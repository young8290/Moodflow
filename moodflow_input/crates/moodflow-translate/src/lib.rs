//! Bounded, offline CC-CEDICT lookup. Uncovered fragments stay visible and unchanged.
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

const MAGIC: &[u8; 8] = b"MFCD\x01\0\0\0";
pub const MAX_SOURCE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_INDEX_BYTES: usize = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 250_000;
const MAX_TERMS: usize = 2_000_000;
const MAX_STRING: usize = 16_384;
const MAX_POSTINGS: usize = 4_000_000;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid_dictionary")]
    Invalid,
    #[error("dictionary_limit")]
    Limit,
    #[error("query_limit")]
    QueryLimit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub traditional: String,
    pub simplified: String,
    pub pinyin: String,
    pub definitions: Vec<String>,
}

#[derive(Clone, Debug)]
struct Term {
    key: String,
    entries: Vec<u32>,
}

pub struct Dictionary {
    entries: Vec<Entry>,
    terms: Vec<Term>,
}

#[derive(Debug, Serialize)]
pub struct Segment {
    pub input: String,
    pub output: String,
    pub covered: bool,
}

#[derive(Debug, Serialize)]
pub struct Lookup {
    pub entries: Vec<Entry>,
    pub segments: Vec<Segment>,
    pub complete: bool,
    pub matched: bool,
    pub entry_count: usize,
}

fn normalize(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn words(s: &str) -> impl Iterator<Item = &str> {
    s.split(|c: char| !c.is_ascii_alphabetic() && c != '\'')
        .filter(|s| !s.is_empty())
}

fn add(map: &mut BTreeMap<String, BTreeSet<u32>>, prefix: &str, key: &str, id: u32) {
    if !key.is_empty() {
        map.entry(format!("{prefix}{key}")).or_default().insert(id);
    }
}

impl Dictionary {
    pub fn compile(source: &str) -> Result<Self, Error> {
        if source.len() > MAX_SOURCE_BYTES {
            return Err(Error::Limit);
        }
        let mut entries = Vec::new();
        let mut map = BTreeMap::new();
        for line in source.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.len() > MAX_STRING || entries.len() >= MAX_ENTRIES {
                return Err(Error::Limit);
            }
            let (traditional, rest) = line.split_once(' ').ok_or(Error::Invalid)?;
            let (simplified, rest) = rest.split_once(" [").ok_or(Error::Invalid)?;
            let (pinyin, definitions) = rest.split_once("] /").ok_or(Error::Invalid)?;
            let definitions: Vec<String> = definitions
                .strip_suffix('/')
                .ok_or(Error::Invalid)?
                .split('/')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
            if traditional.is_empty() || simplified.is_empty() || definitions.is_empty() {
                return Err(Error::Invalid);
            }
            let id = entries.len() as u32;
            add(&mut map, "c:", traditional, id);
            add(&mut map, "c:", simplified, id);
            for definition in &definitions {
                let norm = normalize(definition);
                add(&mut map, "e:", &norm, id);
                if let Some(without_to) = norm.strip_prefix("to ") {
                    add(&mut map, "e:", without_to, id);
                }
                for word in words(&norm) {
                    add(&mut map, "w:", word, id);
                }
            }
            entries.push(Entry {
                traditional: traditional.to_owned(),
                simplified: simplified.to_owned(),
                pinyin: pinyin.to_owned(),
                definitions,
            });
        }
        if entries.is_empty() || map.len() > MAX_TERMS {
            return Err(Error::Invalid);
        }
        if map.values().map(BTreeSet::len).sum::<usize>() > MAX_POSTINGS {
            return Err(Error::Limit);
        }
        Ok(Self {
            entries,
            terms: map
                .into_iter()
                .map(|(key, ids)| Term {
                    key,
                    entries: ids.into_iter().collect(),
                })
                .collect(),
        })
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut body = Vec::new();
        put_u32(&mut body, self.entries.len());
        for entry in &self.entries {
            for s in [&entry.traditional, &entry.simplified, &entry.pinyin] {
                put_string(&mut body, s)?;
            }
            put_u32(&mut body, entry.definitions.len());
            for s in &entry.definitions {
                put_string(&mut body, s)?;
            }
        }
        put_u32(&mut body, self.terms.len());
        let mut previous = "";
        for term in &self.terms {
            let mut shared = previous
                .bytes()
                .zip(term.key.bytes())
                .take_while(|(a, b)| a == b)
                .count();
            while !term.key.is_char_boundary(shared) {
                shared -= 1;
            }
            put_u32(&mut body, shared);
            put_string(&mut body, &term.key[shared..])?;
            put_u32(&mut body, term.entries.len());
            for id in &term.entries {
                put_u32(&mut body, *id as usize);
            }
            previous = &term.key;
        }
        if body.len() + 40 > MAX_INDEX_BYTES {
            return Err(Error::Limit);
        }
        let mut out = Vec::with_capacity(body.len() + 40);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&Sha256::digest(&body));
        out.extend_from_slice(&body);
        Ok(out)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_INDEX_BYTES || bytes.len() < 40 {
            return Err(Error::Limit);
        }
        if &bytes[..8] != MAGIC || Sha256::digest(&bytes[40..])[..] != bytes[8..40] {
            return Err(Error::Invalid);
        }
        let mut r = Reader(&bytes[40..]);
        let count = r.count(MAX_ENTRIES)?;
        if count == 0 {
            return Err(Error::Invalid);
        }
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            let traditional = r.string()?;
            let simplified = r.string()?;
            let pinyin = r.string()?;
            let len = r.count(1024)?;
            if traditional.is_empty() || simplified.is_empty() || len == 0 {
                return Err(Error::Invalid);
            }
            let definitions = (0..len).map(|_| r.string()).collect::<Result<_, _>>()?;
            entries.push(Entry {
                traditional,
                simplified,
                pinyin,
                definitions,
            });
        }
        let count = r.count(MAX_TERMS)?;
        let mut terms: Vec<Term> = Vec::with_capacity(count);
        let mut total = 0;
        for _ in 0..count {
            let shared = r.count(MAX_STRING)?;
            let previous = terms.last().map(|t| t.key.as_str()).unwrap_or("");
            if !previous.is_char_boundary(shared) {
                return Err(Error::Invalid);
            }
            let key = format!("{}{}", &previous[..shared], r.string()?);
            if key.len() > MAX_STRING
                || key.as_str() <= previous
                || !["c:", "e:", "w:"].iter().any(|p| key.starts_with(p))
            {
                return Err(Error::Invalid);
            }
            let len = r.count(MAX_ENTRIES)?;
            total += len;
            if len == 0 || total > MAX_POSTINGS {
                return Err(Error::Limit);
            }
            let mut ids = Vec::with_capacity(len);
            for _ in 0..len {
                let id = r.count(entries.len() - 1)? as u32;
                if ids.last().is_some_and(|last| *last >= id) {
                    return Err(Error::Invalid);
                }
                ids.push(id);
            }
            terms.push(Term { key, entries: ids });
        }
        if !r.0.is_empty() {
            return Err(Error::Invalid);
        }
        Ok(Self { entries, terms })
    }

    fn ids(&self, prefix: &str, query: &str) -> &[u32] {
        let key = format!("{prefix}{query}");
        self.terms
            .binary_search_by(|t| t.key.as_str().cmp(&key))
            .ok()
            .map(|i| self.terms[i].entries.as_slice())
            .unwrap_or(&[])
    }

    pub fn lookup(&self, query: &str) -> Result<Lookup, Error> {
        if query.encode_utf16().count() > 512 || query.contains('\0') {
            return Err(Error::QueryLimit);
        }
        let query = query.trim();
        let english = normalize(query);
        let mut ids = self.ids("c:", query);
        if ids.is_empty() {
            ids = self.ids("e:", &english);
        }
        // A word appearing inside a definition is useful, but is labelled separately
        // by the UI through the original entry definitions; it is never a fake translation.
        if ids.is_empty() && words(&english).count() == 1 {
            ids = self.ids("w:", &english);
        }
        let entries = ids
            .iter()
            .take(30)
            .map(|id| self.entries[*id as usize].clone())
            .collect::<Vec<_>>();
        let segments = if !ids.is_empty() {
            vec![Segment {
                input: query.to_owned(),
                output: self.translation(ids[0], query),
                covered: true,
            }]
        } else if query.chars().any(is_han) {
            self.segment_chinese(query)
        } else {
            query
                .split_inclusive(char::is_whitespace)
                .map(|s| {
                    let word = s.trim();
                    let ids = self.ids("e:", &normalize(word));
                    Segment {
                        input: s.to_owned(),
                        output: ids.first().map_or_else(
                            || s.to_owned(),
                            |id| self.entries[*id as usize].simplified.clone(),
                        ),
                        covered: word.is_empty() || !ids.is_empty(),
                    }
                })
                .collect()
        };
        Ok(Lookup {
            complete: !segments.is_empty() && segments.iter().all(|s| s.covered),
            matched: segments
                .iter()
                .any(|s| s.covered && !s.input.trim().is_empty()),
            entries,
            segments,
            entry_count: self.entries.len(),
        })
    }

    fn translation(&self, id: u32, query: &str) -> String {
        let entry = &self.entries[id as usize];
        if query.chars().any(is_han) {
            entry.definitions[0].clone()
        } else {
            entry.simplified.clone()
        }
    }

    fn segment_chinese(&self, query: &str) -> Vec<Segment> {
        let boundaries: Vec<usize> = query
            .char_indices()
            .map(|(i, _)| i)
            .chain([query.len()])
            .collect();
        let mut out: Vec<Segment> = Vec::new();
        let mut start = 0;
        while start + 1 < boundaries.len() {
            let longest = (start + 32).min(boundaries.len() - 1);
            let found = (start + 1..=longest).rev().find_map(|end| {
                let word = &query[boundaries[start]..boundaries[end]];
                self.ids("c:", word).first().map(|id| (end, *id))
            });
            let (end, covered, output) = match found {
                Some((end, id)) => (end, true, self.entries[id as usize].definitions[0].clone()),
                None => {
                    let s = &query[boundaries[start]..boundaries[start + 1]];
                    (start + 1, s.chars().all(char::is_whitespace), s.to_owned())
                }
            };
            let input = query[boundaries[start]..boundaries[end]].to_owned();
            if !covered && out.last().is_some_and(|s| !s.covered) {
                let last = out.last_mut().unwrap();
                last.input.push_str(&input);
                last.output.push_str(&output);
            } else {
                out.push(Segment {
                    input,
                    output,
                    covered,
                });
            }
            start = end;
        }
        out
    }
}

fn is_han(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x9fff | 0x20000..=0x323af | 0xf900..=0xfaff)
}

fn put_u32(out: &mut Vec<u8>, value: usize) {
    out.extend_from_slice(&(value as u32).to_le_bytes());
}
fn put_string(out: &mut Vec<u8>, value: &str) -> Result<(), Error> {
    if value.len() > MAX_STRING {
        return Err(Error::Limit);
    }
    put_u32(out, value.len());
    out.extend_from_slice(value.as_bytes());
    Ok(())
}
struct Reader<'a>(&'a [u8]);
impl Reader<'_> {
    fn count(&mut self, max: usize) -> Result<usize, Error> {
        let bytes = self.0.get(..4).ok_or(Error::Invalid)?;
        let n = u32::from_le_bytes(bytes.try_into().map_err(|_| Error::Invalid)?) as usize;
        self.0 = &self.0[4..];
        if n > max { Err(Error::Limit) } else { Ok(n) }
    }
    fn string(&mut self) -> Result<String, Error> {
        let n = self.count(MAX_STRING)?;
        let s = std::str::from_utf8(self.0.get(..n).ok_or(Error::Invalid)?)
            .map_err(|_| Error::Invalid)?
            .to_owned();
        self.0 = &self.0[n..];
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SOURCE: &str = "# test dictionary\n中國 中国 [Zhong1 guo2] /China/\n你好 你好 [ni3 hao3] /hello/hi/\n書 书 [shu1] /book/to write/\n圖書 图书 [tu2 shu1] /books and magazines/\n";

    #[test]
    fn indexed_roundtrip_is_deterministic_and_bidirectional() {
        let dictionary = Dictionary::compile(SOURCE).unwrap();
        let bytes = dictionary.to_bytes().unwrap();
        assert_eq!(
            bytes,
            Dictionary::compile(SOURCE).unwrap().to_bytes().unwrap()
        );
        let dictionary = Dictionary::from_bytes(&bytes).unwrap();
        for query in ["中国", "中國", " China "] {
            let result = dictionary.lookup(query).unwrap();
            assert!(result.complete);
            assert_eq!(result.entries[0].simplified, "中国");
        }
        assert_eq!(
            dictionary.lookup("write").unwrap().entries[0].simplified,
            "书"
        );
        assert_eq!(
            dictionary.lookup("magazines").unwrap().entries[0].simplified,
            "图书"
        );
    }

    #[test]
    fn longest_match_keeps_unknown_fragments_and_empty_is_not_complete() {
        let dictionary = Dictionary::compile(SOURCE).unwrap();
        let result = dictionary.lookup("你好🫧未收录中国").unwrap();
        assert!(!result.complete);
        assert!(result.matched);
        assert_eq!(result.segments[1].input, "🫧未收录");
        assert_eq!(result.segments[1].output, "🫧未收录");
        assert!(!result.segments[1].covered);
        assert_eq!(result.segments[2].output, "China");
        assert!(!dictionary.lookup("").unwrap().complete);
        assert!(dictionary.lookup(&"x".repeat(513)).is_err());
        assert_eq!(
            dictionary.lookup("hello unlisted").unwrap().segments[1].output,
            "unlisted"
        );
    }

    #[test]
    fn decoder_rejects_tampering_truncation_and_rehashed_invalid_prefix() {
        let mut bytes = Dictionary::compile(SOURCE).unwrap().to_bytes().unwrap();
        for len in 0..bytes.len() {
            assert!(Dictionary::from_bytes(&bytes[..len]).is_err());
        }
        bytes[48] ^= 1;
        assert!(Dictionary::from_bytes(&bytes).is_err());
        let mut body = Vec::new();
        put_u32(&mut body, 1);
        for s in ["中", "中", "zhong1"] {
            put_string(&mut body, s).unwrap();
        }
        put_u32(&mut body, 1);
        put_string(&mut body, "middle").unwrap();
        put_u32(&mut body, 1);
        put_u32(&mut body, 999); // Prefix beyond the preceding key, despite a valid checksum.
        put_string(&mut body, "c:").unwrap();
        put_u32(&mut body, 1);
        put_u32(&mut body, 0);
        let mut corrupt = MAGIC.to_vec();
        corrupt.extend_from_slice(&Sha256::digest(&body));
        corrupt.extend_from_slice(&body);
        assert!(Dictionary::from_bytes(&corrupt).is_err());
    }
}
