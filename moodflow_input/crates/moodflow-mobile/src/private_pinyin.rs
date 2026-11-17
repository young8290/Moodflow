//! Factory-only mobile query path. It has no coordinator, store or user configuration reference.

use std::path::Path;
use std::sync::OnceLock;

use moodflow_engine::{ConvertOptions, Engine, EngineManager, PinyinEngine};

#[derive(Default)]
pub(crate) struct FactoryPinyin(OnceLock<Option<PinyinEngine>>);

impl FactoryPinyin {
    /// May prepare a cache from factory data, before accepting any editor input.
    pub(crate) fn prepare(&self, data_dir: &Path) -> bool {
        self.0
            .get_or_init(|| EngineManager::load_factory_pinyin(data_dir))
            .is_some()
    }

    pub(crate) fn candidates(&self, spelling: &str) -> Vec<String> {
        if spelling.is_empty()
            || spelling.len() > 48
            || spelling
                .split('\'')
                .any(|part| part.is_empty() || !part.bytes().all(|ch| ch.is_ascii_lowercase()))
        {
            return Vec::new();
        }
        let Some(Some(engine)) = self.0.get() else {
            return Vec::new();
        };
        // Do not let query-dependent diagnostics reach a process logger, even in debug builds.
        tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || {
            engine
                .convert_with_opts(
                    spelling,
                    48,
                    ConvertOptions {
                        require_full_match: true,
                        ..Default::default()
                    },
                )
                .map(|result| {
                    result
                        .candidates
                        .into_iter()
                        .map(|item| item.text)
                        .collect()
                })
                .unwrap_or_default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn factory_lookup_works_without_a_coordinator_or_user_store() {
        let root =
            std::env::temp_dir().join(format!("moodflow-factory-input-{}", std::process::id()));
        let data = root.join("data");
        let subdir = data.join("schemas/pinyin");
        fs::create_dir_all(&subdir).unwrap();
        let source = subdir.join("rime_frost.dict.yaml");
        let contents =
            "---\nname: private_test\n...\n你\tni\t200\n好\thao\t150\n你好\tni hao\t500\n";
        fs::write(&source, contents).unwrap();

        let engine = FactoryPinyin::default();
        assert!(engine.candidates("ni").is_empty());
        assert!(engine.prepare(&data));
        assert!(
            engine
                .candidates("ni'hao")
                .iter()
                .any(|word| word == "你好")
        );
        assert!(engine.candidates("NI").is_empty());
        assert!(engine.candidates("ni''hao").is_empty());
        assert!(!root.join("userdata.redb").exists());
        assert_eq!(fs::read_to_string(&source).unwrap(), contents);
        drop(engine);
        fs::remove_dir_all(root).unwrap();
    }
}
