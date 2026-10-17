use super::*;
use moodflow_plugin_api::Manifest;

const HOST: &str = "0.130.0";

fn manifest(contributes: &str, caps: &str) -> Manifest {
    let text = format!(
        r#"
id = "dev.test.plugin"
name = "测试"
version = "1.0.0"
sdk = 1
min_host_version = "0.1.0"
type = "wasm"
entry = "plugin.wasm"
platforms = ["windows", "android"]
license = "MIT"
[contributes]
{contributes}
[capabilities]
{caps}
"#
    );
    Manifest::from_toml(&text, HOST).unwrap()
}

/// 公共部分：一页内存、从 8192 起的递增分配器。
fn module(imports: &str, body: &str) -> Vec<u8> {
    let src = format!(
        r#"(module
  {imports}
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 8192))
  (func (export "mf_alloc") (param $n i32) (result i32)
    (local $p i32)
    (local.set $p (global.get $heap))
    (global.set $heap (i32.add (global.get $heap) (local.get $n)))
    (local.get $p))
  {body}
)"#
    );
    wat::parse_str(&src).unwrap()
}

/// 返回 1024 处一段常量 JSON 的函数体。
fn returns_const(export: &str, json: &str) -> String {
    let escaped = json.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        r#"(data (i32.const 1024) "{escaped}")
  (func (export "{export}") (param i32 i32) (result i64)
    (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const {len})))"#,
        len = json.len()
    )
}

fn load(m: Manifest, wasm: &[u8]) -> Result<PluginInstance, LoadError> {
    PluginInstance::load(m, wasm, Limits::default(), Box::new(MemoryHost::default()))
}

const OK: InputContext = InputContext {
    private_field: false,
    secure_desktop: false,
    incognito: false,
    app_blocked: false,
};

#[test]
fn candidate_plugin_returns_candidates() {
    let json = r#"[{"text":"😀","comment":"笑","weight":120}]"#;
    let wasm = module("", &returns_const("mf_candidates", json));
    let mut p = load(manifest("candidate_provider = {}", ""), &wasm).unwrap();
    let got = p.candidates(&OK, "xiao").unwrap();
    assert_eq!(
        got,
        vec![PluginCandidate {
            text: "😀".into(),
            comment: "笑".into(),
            weight: 120
        }]
    );
}

#[test]
fn private_contexts_never_reach_the_plugin() {
    // 插件一被调用就会死循环耗尽步数；返回空且不报错，说明根本没调用。
    let wasm = module(
        "",
        r#"(func (export "mf_candidates") (param i32 i32) (result i64) (loop $l (br $l)) (i64.const 0))"#,
    );
    let mut p = load(manifest("candidate_provider = {}", ""), &wasm).unwrap();
    for ctx in [
        InputContext {
            private_field: true,
            ..OK
        },
        InputContext {
            secure_desktop: true,
            ..OK
        },
        InputContext {
            incognito: true,
            ..OK
        },
        InputContext {
            app_blocked: true,
            ..OK
        },
    ] {
        assert_eq!(p.candidates(&ctx, "mima"), Ok(vec![]));
    }
    assert!(!p.is_disabled());
}

#[test]
fn runaway_plugin_runs_out_of_fuel_and_gets_disabled() {
    let wasm = module(
        "",
        r#"(func (export "mf_candidates") (param i32 i32) (result i64) (loop $l (br $l)) (i64.const 0))"#,
    );
    let mut p = load(manifest("candidate_provider = {}", ""), &wasm).unwrap();
    for _ in 0..3 {
        assert_eq!(p.candidates(&OK, "a"), Err(CallError::OutOfFuel));
    }
    assert!(p.is_disabled());
    assert_eq!(p.candidates(&OK, "a"), Err(CallError::Disabled));
    p.reenable();
    assert_eq!(p.candidates(&OK, "a"), Err(CallError::OutOfFuel));
}

#[test]
fn memory_cannot_grow_past_the_limit() {
    // 申请 1000 页（约 64 MB）：失败返回 -1 → 输出 "[]"；成功则输出一个候选。
    let wasm = module(
        "",
        r#"(data (i32.const 1024) "[]")
  (data (i32.const 1100) "[{\"text\":\"grew\"}]")
  (func (export "mf_candidates") (param i32 i32) (result i64)
    (if (result i64) (i32.eq (memory.grow (i32.const 1000)) (i32.const -1))
      (then (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const 2)))
      (else (i64.or (i64.shl (i64.const 1100) (i64.const 32)) (i64.const 17)))))"#,
    );
    let mut p = load(manifest("candidate_provider = {}", ""), &wasm).unwrap();
    assert_eq!(p.candidates(&OK, "a"), Ok(vec![]));
}

#[test]
fn storage_round_trip_through_host() {
    // transform：把入参原样存进 key "k"，再读回来作为结果（入参就是 {"text": ...}）。
    let wasm = module(
        r#"(import "mf" "storage_get" (func $get (param i32 i32 i32 i32) (result i32)))
  (import "mf" "storage_set" (func $set (param i32 i32 i32 i32) (result i32)))"#,
        r#"(data (i32.const 512) "k")
  (func (export "mf_transform") (param $p i32) (param $n i32) (result i64)
    (local $len i32)
    (drop (call $set (i32.const 512) (i32.const 1) (local.get $p) (local.get $n)))
    (local.set $len (call $get (i32.const 512) (i32.const 1) (i32.const 32768) (i32.const 4096)))
    (i64.or (i64.shl (i64.const 32768) (i64.const 32)) (i64.extend_i32_u (local.get $len))))"#,
    );
    let mut p = load(manifest("commit_transform = {}", "storage = true"), &wasm).unwrap();
    assert_eq!(p.transform(&OK, "你好").unwrap(), "你好");
}

#[test]
fn importing_an_undeclared_capability_is_rejected_at_load() {
    let wasm = module(
        r#"(import "mf" "storage_set" (func $set (param i32 i32 i32 i32) (result i32)))"#,
        &returns_const("mf_candidates", "[]"),
    );
    assert_eq!(
        load(manifest("candidate_provider = {}", ""), &wasm).err(),
        Some(LoadError::MissingCapability {
            import: "storage_set".into(),
            capability: "storage"
        })
    );
}

#[test]
fn unknown_imports_are_rejected() {
    let wasm = module(
        r#"(import "env" "fd_write" (func $w (param i32) (result i32)))"#,
        &returns_const("mf_candidates", "[]"),
    );
    assert!(matches!(
        load(manifest("candidate_provider = {}", ""), &wasm),
        Err(LoadError::UnknownImport(_))
    ));
}

#[test]
fn declared_extension_point_must_be_exported() {
    let wasm = module("", &returns_const("mf_candidates", "[]"));
    assert_eq!(
        load(
            manifest("candidate_provider = {}\ncommit_transform = {}", ""),
            &wasm
        )
        .err(),
        Some(LoadError::MissingExport("mf_transform"))
    );
}

#[test]
fn calling_an_undeclared_extension_point_fails() {
    let wasm = module("", &returns_const("mf_candidates", "[]"));
    let mut p = load(manifest("candidate_provider = {}", ""), &wasm).unwrap();
    assert_eq!(p.transform(&OK, "x"), Err(CallError::NotContributed));
    assert_eq!(p.cmd("rate", &[]), Err(CallError::NotContributed));
}

#[test]
fn cmd_only_answers_declared_function_names() {
    let wasm = module("", &returns_const("mf_cmd", r#"{"value":"42"}"#));
    let mut p = load(manifest("cmdbar_functions = [\"answer\"]", ""), &wasm).unwrap();
    assert_eq!(p.cmd("answer", &[]), Ok("42".into()));
    assert_eq!(p.cmd("other", &[]), Err(CallError::NotContributed));
}

#[test]
fn malformed_output_counts_as_a_failure() {
    let wasm = module("", &returns_const("mf_candidates", "not json"));
    let mut p = load(manifest("candidate_provider = {}", ""), &wasm).unwrap();
    for _ in 0..3 {
        assert_eq!(p.candidates(&OK, "a"), Err(CallError::BadOutput));
    }
    assert!(p.is_disabled());
}

#[test]
fn notify_requires_capability_and_reaches_host() {
    let wasm = module(
        r#"(import "mf" "notify" (func $n (param i32 i32) (result i32)))"#,
        r#"(data (i32.const 600) "hi")
  (data (i32.const 1024) "[]")
  (func (export "mf_candidates") (param i32 i32) (result i64)
    (drop (call $n (i32.const 600) (i32.const 2)))
    (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const 2)))"#,
    );
    assert!(load(manifest("candidate_provider = {}", ""), &wasm).is_err());
    let mut p = load(manifest("candidate_provider = {}", "notify = true"), &wasm).unwrap();
    assert_eq!(p.candidates(&OK, "a"), Ok(vec![]));
}
