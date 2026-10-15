//! `db-grep --db <xinqing.db> --phrases <测试句.txt>`：只读打开数据库，在所有表的所有列里搜索测试句
//! （每行一句，空行和 `#` 开头的行跳过），打印命中的表、列、行号和句子编号。有命中时退出码为 1。
//!
//! 默认放过用户确认的日程标题、地点与待办标题（NFR-PRI-04 的例外），`--strict` 连这些也查。
//! 除整句外还搜每句中 `--fragment` 个字（默认 6）一段的连续片段，原文被截断或改了标点存下时也能查出来；
//! `--fragment 0` 只搜整句。

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser;
use rusqlite::{Connection, OpenFlags};
use xinqing_hub_core::infra::store::grep::{CONFIRMED_USER_TEXT, grep};

#[derive(Parser)]
#[command(about = "在心晴数据库里搜索测试句，验证不存原文（TC-PRI-01）")]
struct Args {
    /// 数据库文件（只读打开，Hub 运行时也可以扫）
    #[arg(long)]
    db: PathBuf,
    /// 测试句文件，每行一句
    #[arg(long)]
    phrases: PathBuf,
    /// 另搜每句中这么多个字一段的片段；0 只搜整句
    #[arg(long, default_value_t = 6)]
    fragment: usize,
    /// 不放过用户确认的日程标题、地点与待办标题
    #[arg(long)]
    strict: bool,
}

fn main() -> Result<ExitCode> {
    let args = Args::parse();
    let text = std::fs::read_to_string(&args.phrases)
        .with_context(|| format!("读不了测试句文件 {}", args.phrases.display()))?;
    let phrases: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect();
    let conn = Connection::open_with_flags(&args.db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("打不开数据库 {}", args.db.display()))?;
    let allow = if args.strict {
        &[][..]
    } else {
        CONFIRMED_USER_TEXT
    };
    let hits = grep(&conn, &phrases, args.fragment, allow)?;
    for h in &hits {
        println!(
            "{}.{}\t行 {}\t第 {} 句：{}",
            h.table,
            h.column,
            h.rowid,
            h.phrase + 1,
            phrases[h.phrase]
        );
    }
    println!("{} 句测试句，命中 {} 处", phrases.len(), hits.len());
    Ok(if hits.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
