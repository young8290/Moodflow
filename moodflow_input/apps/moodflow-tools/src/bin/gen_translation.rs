//! Deterministically compile the pinned, separately licensed CC-CEDICT snapshot.
use anyhow::{Context, ensure};
use moodflow_translate::{Dictionary, MAX_SOURCE_BYTES};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::PathBuf,
};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 4 && args[0] == "--src" && args[2] == "--out",
        "usage: gen_translation --src <data/translate> --out <cedict.mfcd>"
    );
    let source = PathBuf::from(&args[1]);
    let mut manifest_bytes = Vec::new();
    std::fs::File::open(source.join("source.json"))?
        .take(65537)
        .read_to_end(&mut manifest_bytes)?;
    ensure!(manifest_bytes.len() <= 65536, "source manifest limit");
    let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes)?;
    let file = manifest["file"].as_str().context("missing source file")?;
    ensure!(
        PathBuf::from(file).components().count() == 1
            && matches!(
                PathBuf::from(file).components().next(),
                Some(std::path::Component::Normal(_))
            )
            && !file.contains(['/', '\\', ':']),
        "invalid source name"
    );
    let mut compressed = Vec::new();
    std::fs::File::open(source.join(file))?
        .take(MAX_SOURCE_BYTES as u64 + 1)
        .read_to_end(&mut compressed)?;
    ensure!(
        compressed.len() <= MAX_SOURCE_BYTES,
        "compressed source limit"
    );
    ensure!(
        format!("{:x}", Sha256::digest(&compressed)) == manifest["sha256"].as_str().unwrap_or(""),
        "source SHA-256 mismatch"
    );
    let mut text = String::new();
    flate2::read::GzDecoder::new(compressed.as_slice())
        .take(MAX_SOURCE_BYTES as u64 + 1)
        .read_to_string(&mut text)?;
    ensure!(text.len() <= MAX_SOURCE_BYTES, "decompressed source limit");
    ensure!(
        format!("{:x}", Sha256::digest(text.as_bytes()))
            == manifest["uncompressed_sha256"].as_str().unwrap_or(""),
        "decompressed source SHA-256 mismatch"
    );
    let dictionary = Dictionary::compile(&text)?;
    ensure!(
        Some(dictionary.entry_count() as u64) == manifest["entries"].as_u64(),
        "entry count mismatch"
    );
    let bytes = dictionary.to_bytes()?;
    // Decode the exact publication bytes, not just the in-memory builder.
    ensure!(
        Dictionary::from_bytes(&bytes)?.entry_count() == dictionary.entry_count(),
        "index verification failed"
    );
    let output = PathBuf::from(&args[3]);
    let parent = output.parent().context("missing output directory")?;
    std::fs::create_dir_all(parent)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged.write_all(&bytes)?;
    staged.as_file().sync_all()?;
    staged.persist(&output).map_err(|e| e.error)?;
    println!(
        "CC-CEDICT: {} entries, {} bytes, SHA-256 {:x}",
        dictionary.entry_count(),
        bytes.len(),
        Sha256::digest(&bytes)
    );
    Ok(())
}
