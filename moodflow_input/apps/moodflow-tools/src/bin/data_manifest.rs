//! 安装数据清单：`data_manifest build <data目录>` 生成 manifest.json；
//! `data_manifest verify <data目录>` 按清单复核。XSIME 吸收计划 §3.4。
//!
//! 退出码：0 通过；1 校验不一致或数据被污染；2 用法错误。

use std::path::Path;

use moodflow_manifest::{Manifest, Mismatch, build, verify};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match (args.first().map(String::as_str), args.get(1)) {
        (Some("build"), Some(dir)) => cmd_build(Path::new(dir)),
        (Some("verify"), Some(dir)) => cmd_verify(Path::new(dir)),
        _ => {
            eprintln!("用法: data_manifest build|verify <data目录>");
            2
        }
    };
    std::process::exit(code);
}

fn cmd_build(dir: &Path) -> i32 {
    let built = match build(dir) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("生成清单失败: {e}");
            return 1;
        }
    };
    for w in &built.warnings {
        eprintln!("警告: 扩展名不在白名单内（仍计入清单）: {w}");
    }
    if let Err(e) = built.manifest.save(dir) {
        eprintln!("写清单失败: {e}");
        return 1;
    }
    println!("已生成清单：{} 个文件", built.manifest.files.len());
    0
}

fn cmd_verify(dir: &Path) -> i32 {
    let result = Manifest::load(dir).and_then(|m| verify(dir, &m));
    match result {
        Ok(v) if v.is_empty() => {
            println!("数据与清单一致");
            0
        }
        Ok(v) => {
            for m in v {
                match m {
                    Mismatch::Missing(p) => eprintln!("缺失: {p}"),
                    Mismatch::Changed(p) => eprintln!("内容不一致: {p}"),
                    Mismatch::Extra(p) => eprintln!("多余文件: {p}"),
                }
            }
            1
        }
        Err(e) => {
            eprintln!("校验失败: {e}");
            1
        }
    }
}
