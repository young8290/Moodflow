use super::*;

fn zip_of(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default();
        for (name, body) in entries {
            z.start_file(*name, opts).unwrap();
            z.write_all(body.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }
    buf.into_inner()
}

const WUBI_SCHEMA: &str = r#"
schema:
  schema_id: wubi86
  name: 五笔86
engine:
  translators:
    - table_translator
speller:
  max_code_length: 4
translator:
  dictionary: wubi86
"#;

const WUBI_DICT: &str = "---\nname: wubi86\nimport_tables:\n  - extra/words\n...\n你\twq\t100\n";
const EXTRA_DICT: &str = "---\nname: words\n...\n你好\twqvb\t50\n";

const ICE_SCHEMA: &str = r#"
schema:
  schema_id: rime_ice
  name: 雾凇拼音
engine:
  translators:
    - script_translator
    - lua_translator@date
  filters:
    - simplifier
speller:
  algebra:
    - derive/^([zcs])h/$1/
translator:
  dictionary: rime_ice
"#;

const ICE_DICT: &str =
    "---\nname: rime_ice\nimport_tables:\n  - cn_dicts/8105\n  - cn_dicts/missing\n...\n";

fn convert_zip(entries: &[(&str, &str)]) -> Result<Conversion, RimeImportError> {
    let (files, sha, pre) = read_zip_bytes(&zip_of(entries))?;
    convert(&files, sha, pre)
}

fn file<'a>(s: &'a ConvertedScheme, name: &str) -> &'a [u8] {
    &s.files
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("{name} 不在包里"))
        .1
}

#[test]
fn codetable_scheme_with_import_tables_becomes_multiple_dictionaries() {
    let c = convert_zip(&[
        ("wubi86.schema.yaml", WUBI_SCHEMA),
        ("wubi86.dict.yaml", WUBI_DICT),
        ("extra/words.dict.yaml", EXTRA_DICT),
    ])
    .unwrap();
    assert_eq!(c.schemes.len(), 1);
    let s = &c.schemes[0];
    assert_eq!(s.id, "rime_wubi86");
    assert_eq!(s.engine, EngineKind::Codetable);
    let toml_text = String::from_utf8(file(s, "rime_wubi86.schema.toml").to_vec()).unwrap();
    let parsed: moodflow_config::schema::Schema = toml::from_str(&toml_text).expect(&toml_text);
    assert_eq!(parsed.dictionaries.len(), 2, "{toml_text}");
    assert!(toml_text.contains("max_code_length = 4"));
    assert!(toml_text.contains("rime_wubi86/extra/words.dict.yaml"));
    assert_eq!(
        file(s, "rime_wubi86/wubi86.dict.yaml"),
        WUBI_DICT.as_bytes(),
        "词库原样搬运"
    );
    assert!(c.diagnostics.contains(&Diagnostic::IdRenamed {
        from: "wubi86".into(),
        to: "rime_wubi86".into()
    }));
}

#[test]
fn pinyin_scheme_reports_what_could_not_be_converted() {
    let c = convert_zip(&[
        ("rime-ice-main/rime_ice.schema.yaml", ICE_SCHEMA),
        ("rime-ice-main/rime_ice.dict.yaml", ICE_DICT),
        (
            "rime-ice-main/cn_dicts/8105.dict.yaml",
            "---\nname: 8105\n...\n的\tde\n",
        ),
        ("rime-ice-main/lua/date.lua", "-- lua"),
        ("rime-ice-main/opencc/emoji.json", "{}"),
    ])
    .unwrap();
    let s = &c.schemes[0];
    assert_eq!(s.id, "rime_ice", "已有 rime_ 前缀不重复加");
    assert_eq!(s.engine, EngineKind::Pinyin);
    let d = &c.diagnostics;
    assert!(d.contains(&Diagnostic::LuaIgnored {
        schema: "rime_ice".into()
    }));
    assert!(d.contains(&Diagnostic::FiltersIgnored {
        schema: "rime_ice".into()
    }));
    assert!(d.contains(&Diagnostic::SpellerAlgebraIgnored {
        schema: "rime_ice".into()
    }));
    assert!(d.contains(&Diagnostic::MissingDictionary {
        schema: "rime_ice".into(),
        name: "cn_dicts/missing".into()
    }));
    assert!(
        d.iter()
            .any(|x| matches!(x, Diagnostic::UnsupportedComponent { kind: "lua", .. }))
    );
    assert!(
        d.iter()
            .any(|x| matches!(x, Diagnostic::UnsupportedComponent { kind: "opencc", .. }))
    );
    let toml_text = String::from_utf8(file(s, "rime_ice.schema.toml").to_vec()).unwrap();
    assert!(toml_text.contains("rime_pinyin"));
    let parsed: moodflow_config::schema::Schema = toml::from_str(&toml_text).expect(&toml_text);
    assert_eq!(parsed.dictionaries.len(), 2);
}

#[test]
fn private_state_and_junk_are_ignored() {
    let (files, _, diags) = read_zip_bytes(&zip_of(&[
        ("x.schema.yaml", WUBI_SCHEMA),
        ("__MACOSX/._x.schema.yaml", "junk"),
        ("x.userdb/LOG", "private"),
        ("build/x.prism.bin", "bin"),
        ("sync/abc/x.userdb.txt", "private"),
        ("installation.yaml", "id"),
        ("user.yaml", "u"),
    ]))
    .unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(diags.len(), 6);
}

#[test]
fn unsafe_paths_are_rejected() {
    let err = read_zip_bytes(&zip_of(&[("../evil.schema.yaml", "x")])).unwrap_err();
    assert!(matches!(err, RimeImportError::UnsafePath(_)));
}

#[test]
fn multiple_unrelated_roots_are_a_diagnosable_error() {
    let err = convert_zip(&[
        ("a/x.schema.yaml", WUBI_SCHEMA),
        ("b/y.schema.yaml", WUBI_SCHEMA),
    ])
    .unwrap_err();
    assert_eq!(
        err,
        RimeImportError::MultipleRoots(vec!["a/".into(), "b/".into()])
    );
}

#[test]
fn no_schema_and_nothing_convertible() {
    assert_eq!(
        convert_zip(&[("a.dict.yaml", WUBI_DICT)]).unwrap_err(),
        RimeImportError::NoSchema
    );
    assert_eq!(
        convert_zip(&[("x.schema.yaml", WUBI_SCHEMA)]).unwrap_err(),
        RimeImportError::NothingConvertible,
        "主词库缺失时跳过方案"
    );
}

#[test]
fn same_zip_gives_same_hash_and_output() {
    let entries = [
        ("wubi86.schema.yaml", WUBI_SCHEMA),
        ("wubi86.dict.yaml", WUBI_DICT),
    ];
    let a = convert_zip(&entries).unwrap();
    let b = convert_zip(&entries).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.source_sha256.len(), 64);
}

#[test]
fn written_wpkg_installs_with_existing_importer() {
    let c = convert_zip(&[
        ("wubi86.schema.yaml", WUBI_SCHEMA),
        ("wubi86.dict.yaml", WUBI_DICT),
        ("extra/words.dict.yaml", EXTRA_DICT),
    ])
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("x.wpkg");
    write_wpkg(&c.schemes[0], &pkg).unwrap();
    let user = dir.path().join("user");
    std::fs::create_dir_all(&user).unwrap();
    let r = crate::scheme::import_package(&pkg, &user, crate::merge::Strategy::Replace).unwrap();
    assert_eq!(r.schema_ids, vec!["rime_wubi86".to_string()]);
    assert!(user.join("rime_wubi86/extra/words.dict.yaml").exists());
}

#[test]
fn text_dictionary_handles_bom_swapped_columns_and_bad_lines() {
    let src = "\u{FEFF}# 注释\n你好\tnihao\t10\nzaijian\t再见\n坏行\n\n";
    let (out, skipped) = text_to_rime_dict(src.as_bytes(), "我的 词库").unwrap();
    assert_eq!(skipped, 1);
    assert!(out.contains("name: ____"), "名字清理为 ASCII 标识：{out}");
    assert!(out.contains("你好\tnihao\t10\n"));
    assert!(out.contains("再见\tzaijian\n"));
}

#[test]
fn text_dictionary_decodes_utf16_and_rejects_garbage() {
    let mut le = vec![0xFF, 0xFE];
    for u in "词\tci\n".encode_utf16() {
        le.extend_from_slice(&u.to_le_bytes());
    }
    assert!(text_to_rime_dict(&le, "d").unwrap().0.contains("词\tci"));
    assert_eq!(
        text_to_rime_dict(&[0xC3, 0x28], "d").unwrap_err(),
        RimeImportError::UnsupportedEncoding
    );
    assert_eq!(
        text_to_rime_dict(b"# only\n", "d").unwrap_err(),
        RimeImportError::EmptyDictionary
    );
}

#[test]
fn old_mac_line_endings_are_normalized() {
    let (out, _) = text_to_rime_dict("你好\tnihao\r再见\tzaijian\r".as_bytes(), "d").unwrap();
    assert!(
        out.contains("你好\tnihao\n") && out.contains("再见\tzaijian\n"),
        "{out}"
    );
    let dict = "---\rname: x\rimport_tables:\r  - extra/words\r...\r";
    assert_eq!(
        dict_imports(dict.as_bytes()),
        vec!["extra/words".to_string()]
    );
}

#[test]
fn mobile_installs_only_chosen_scheme_and_removes_runtime_import_references() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("rime.zip");
    let schemas = root.path().join("schemas");
    std::fs::write(
        &source,
        zip_of(&[
            ("wrapper/wubi86.schema.yaml", WUBI_SCHEMA),
            ("wrapper/wubi86.dict.yaml", WUBI_DICT),
            ("wrapper/extra/words.dict.yaml", EXTRA_DICT),
            ("wrapper/ice.schema.yaml", ICE_SCHEMA),
            ("wrapper/rime_ice.dict.yaml", ICE_DICT),
            (
                "wrapper/cn_dicts/8105.dict.yaml",
                "---\nname: 8105\n...\n你好\tni hao\t20\n",
            ),
            ("wrapper/user.yaml", "private"),
        ]),
    )
    .unwrap();
    let preview = crate::rime_install::read_archive(&source).unwrap();
    assert_eq!(preview.schemes.len(), 2);
    assert!(!schemas.exists());
    assert!(
        preview
            .diagnostics
            .iter()
            .any(|d| matches!(d, Diagnostic::LuaIgnored { .. }))
    );
    crate::rime_install::install(&preview, "rime_ice", &schemas).unwrap();

    let schema_path = schemas.join("rime_ice.schema.toml");
    let text = std::fs::read_to_string(&schema_path).unwrap();
    let parsed: moodflow_config::schema::Schema = toml::from_str(&text).unwrap();
    assert_eq!(parsed.dictionaries.len(), 2);
    let main = std::fs::read_to_string(schemas.join(&parsed.dictionaries[0].path)).unwrap();
    let child = std::fs::read_to_string(schemas.join(&parsed.dictionaries[1].path)).unwrap();
    assert_eq!(main, "---\nname: rime_ice\n...\n");
    assert!(child.ends_with("你好\tni hao\t20\n"));
    let (entries, _) = moodflow_dict::codetable::parse_rime_entries_parallel(
        schemas.join(&parsed.dictionaries[1].path),
        false,
    )
    .unwrap();
    assert!(!entries.is_empty());
    assert!(!schemas.join("rime_wubi86.schema.toml").exists());
    assert!(crate::rime_install::install(&preview, "rime_ice", &schemas).is_err());
    assert_eq!(std::fs::read_to_string(&schema_path).unwrap(), text);
}

#[test]
fn failed_mobile_install_leaves_no_selectable_scheme() {
    let converted = convert_zip(&[
        ("wubi86.schema.yaml", WUBI_SCHEMA),
        ("wubi86.dict.yaml", "---\nname: wubi86\n...\n"),
    ])
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let error = crate::rime_install::install(&converted, "rime_wubi86", root.path()).unwrap_err();
    assert!(error.to_string().contains("没有可用条目"));
    assert!(!root.path().join("rime_wubi86.schema.toml").exists());
}

#[test]
fn shared_dictionary_counts_against_every_converted_scheme() {
    let second = WUBI_SCHEMA.replace("schema_id: wubi86", "schema_id: other");
    let zip = zip_of(&[
        ("wubi86.schema.yaml", WUBI_SCHEMA),
        ("other.schema.yaml", &second),
        ("wubi86.dict.yaml", WUBI_DICT),
        ("extra/words.dict.yaml", EXTRA_DICT),
    ]);
    let (files, sha, diagnostics) = read_zip_bytes(&zip).unwrap();
    let one_scheme = WUBI_DICT.len() + EXTRA_DICT.len();
    assert!(matches!(
        convert_with_budget(&files, sha, diagnostics, one_scheme),
        Err(RimeImportError::TooLarge)
    ));
}
