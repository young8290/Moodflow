//! NFR-PRI-04（TC-PRI-01）与 NFR-PRI-03（TC-STA-05）的自动化部分：把 50 句特征句当作经输入法上屏的文字
//! （同意了 ③，核心发 `commit.text`），连同打字节奏一起喂给实时感知和日程识别，再用 `db-grep` 的同一份
//! 扫描逻辑查全库——除用户确认的日程标题、地点与待办标题外，不应搜到任何一句（连 6 个字的片段也不行）；
//! 发给 Jev 的 Q-STATE 请求里不应有任何汉字。
//!
//! 日程识别让 Jev 对每句都说“是”、大模型不可用，于是命中初筛的句子都会走本地规则抽取并存为待确认，
//! 覆盖原句离开内存最多的那条路径。时间用 tokio 的暂停时钟。

use std::ops::Deref;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{Local, TimeZone};
use futures::stream::BoxStream;
use tokio::sync::mpsc;
use xinqing_hub_core::bus;
use xinqing_hub_core::domain::features::Baseline;
use xinqing_hub_core::domain::schedule::{CandidateKind, ExtractPrompts, ScheduleRecognizer};
use xinqing_hub_core::domain::status::StatusSnapshot;
use xinqing_hub_core::domain::validate::BannedWords;
use xinqing_hub_core::infra::clock::ManualClock;
use xinqing_hub_core::infra::gateway::{
    AiError, AiGateway, Answer, CompleteRequest, CompleteResponse, Delta, GatewayHealth,
    JudgeRequest, JudgeResponse, Question,
};
use xinqing_hub_core::infra::store::Db;
use xinqing_hub_core::infra::store::grep::{CONFIRMED_USER_TEXT, grep};
use xinqing_hub_core::infra::templates::{AppCategories, BaselineDefault, TemplateDirs};
use xinqing_hub_core::infra::xqp::{LinkEvent, PeerInfo, XqpHandle};
use xinqing_hub_core::pipeline::StatePipeline;
use xinqing_hub_core::schedule::{Ready, SchedulePort, ScheduleService};
use xinqing_hub_core::sense::{Sense, SensePort, WindowRecord};
use xqp::{KeyKind, KeySrc, OpenTarget, Up};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn sentences() -> Vec<String> {
    std::fs::read_to_string(root().join("eval/datasets/pri_sentences.txt"))
        .unwrap()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect()
}

/// 外壳的替身：感知与日程识别共用一个文件数据库，写法与 `src-tauri` 的 `ShellPort` 相同。
struct Shell {
    db: Mutex<Db>,
    windows: Mutex<u32>,
    ready: Mutex<u32>,
}

impl SensePort for Shell {
    fn update_status(&self, f: &mut dyn FnMut(&mut StatusSnapshot) -> bool) {
        f(&mut StatusSnapshot::default());
    }
    fn open(&self, _: OpenTarget) {}
    fn save_window(&self, rec: &WindowRecord<'_>) {
        let db = self.db.lock().unwrap();
        let w = rec.window;
        let id = db
            .insert_window(rec.start_ms, rec.end_ms, w.app_cat, &w.features, &w.hints)
            .unwrap();
        let mood = db
            .insert_mood_state(
                rec.end_ms,
                Some(id),
                rec.fusion.cand,
                rec.fusion.shown,
                rec.fusion.source,
            )
            .unwrap();
        if let Some(j) = rec.judged {
            db.set_mood_jev(mood, j).unwrap();
        }
        *self.windows.lock().unwrap() += 1;
    }
    fn note(&self, _: &str) {}
    fn jev_allowed(&self) -> bool {
        true
    }
}

impl SchedulePort for Shell {
    fn allowed(&self) -> bool {
        true
    }
    fn daily_cap(&self) -> usize {
        100
    }
    fn default_offsets(&self) -> Vec<i64> {
        vec![600]
    }
    fn db(&self) -> Box<dyn Deref<Target = Db> + '_> {
        Box::new(self.db.lock().unwrap())
    }
    fn tip(&self, _: CandidateKind) {}
    fn detected(&self, _: u32, _: CandidateKind) {}
    fn ready(&self, _: &Ready) {
        *self.ready.lock().unwrap() += 1;
    }
}

/// Jev 对 Q-STATE 回“流畅”、对 Q-PLAN / Q-TODO 都说“是”；大模型不可用。记下所有 Jev 请求。
#[derive(Default)]
struct Ai {
    judged: Mutex<Vec<JudgeRequest>>,
}

#[async_trait]
impl AiGateway for Ai {
    async fn judge(&self, req: JudgeRequest) -> Result<JudgeResponse, AiError> {
        self.judged.lock().unwrap().push(req.clone());
        let answers = req
            .questions
            .iter()
            .map(|&q| {
                let a = match q {
                    Question::State => Answer::Choice {
                        probs: [("fluent".to_string(), 0.9)].into(),
                        choice: "fluent".into(),
                    },
                    Question::Valence => Answer::Score { score: 2.0 },
                    _ => Answer::Noul { p: 0.95 },
                };
                (q, a)
            })
            .collect();
        Ok(JudgeResponse {
            model: "jev-test".into(),
            latency_ms: 5,
            answers,
        })
    }
    async fn complete(&self, _: CompleteRequest) -> Result<CompleteResponse, AiError> {
        Err(AiError::ModelUnavailable)
    }
    async fn stream(
        &self,
        _: CompleteRequest,
    ) -> Result<BoxStream<'static, Result<Delta, AiError>>, AiError> {
        Err(AiError::ModelUnavailable)
    }
    fn health(&self) -> GatewayHealth {
        GatewayHealth {
            jev: true,
            llm: false,
        }
    }
}

fn key(ts: u64) -> Up {
    Up::Key {
        ts,
        seq: None,
        kind: KeyKind::Letter,
        vk: Some(b'A'),
        in_comp: true,
        src: KeySrc::Core,
        eaten: None,
        del_committed: None,
    }
}

#[tokio::test(start_paused = true)]
async fn typed_sentences_never_reach_the_database() {
    let dir = std::env::temp_dir().join(format!("xq-pri-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("xinqing.db");
    let _ = std::fs::remove_file(&path);

    let dirs = TemplateDirs::factory_only(root().join("hub_templates"));
    let start = Local.with_ymd_and_hms(2026, 10, 12, 10, 0, 0).unwrap();
    let clock = Arc::new(ManualClock::new(start));
    let shell = Arc::new(Shell {
        db: Mutex::new(Db::open(&path).unwrap()),
        windows: Mutex::new(0),
        ready: Mutex::new(0),
    });
    let ai = Arc::new(Ai::default());
    let (bus_tx, _) = bus::channel();

    let schedule = Arc::new(ScheduleService::new(
        shell.clone(),
        ai.clone(),
        clock.clone(),
        ScheduleRecognizer::load(&dirs).unwrap(),
        ExtractPrompts::load(&dirs).unwrap(),
        Arc::new(BannedWords::load(&dirs).unwrap()),
    ));
    let schedule_task = tokio::spawn(schedule.run(bus_tx.subscribe()));

    let base = Baseline::from_defaults(&BaselineDefault::load(&dirs).unwrap());
    let pipeline = StatePipeline::new(base, AppCategories::load(&dirs).unwrap(), start);
    let sense = Sense::new(
        pipeline,
        shell.clone(),
        XqpHandle::pair().0,
        bus_tx.clone(),
        clock.clone(),
    )
    .with_gateway(ai.clone());
    let (link_tx, link_rx) = mpsc::channel(64);
    let (cmd_tx, cmd_rx) = mpsc::channel(4);
    let sense_task = tokio::spawn(sense.run(link_rx, cmd_rx));

    link_tx
        .send(LinkEvent::Connected(PeerInfo {
            ime_ver: "test".into(),
            session: "s1".into(),
            caps: vec!["core_keys".into()],
        }))
        .await
        .unwrap();
    let sentences = sentences();
    assert_eq!(sentences.len(), 50);
    let mut ts = 1_000;
    for s in &sentences {
        let chars = s.chars().count() as u32;
        // 拼音大约每字 3 键，160 ms 一键
        for _ in 0..chars * 3 {
            link_tx.send(LinkEvent::Up(key(ts))).await.unwrap();
            tokio::time::sleep(Duration::from_millis(160)).await;
            ts += 160;
        }
        let commit = Up::Commit {
            ts,
            seq: None,
            chars,
            keystrokes: chars * 3,
            cand_pos: 1,
            src: "core".into(),
            text: Some(s.clone()),
            truncated: None,
        };
        link_tx.send(LinkEvent::Up(commit)).await.unwrap();
        // 停顿 3 秒结束这个窗口
        tokio::time::sleep(Duration::from_secs(3)).await;
        ts += 3_000;
    }
    // 等最后的 Jev 判断、日程抽取写完
    tokio::time::sleep(Duration::from_secs(30)).await;
    drop(cmd_tx);
    sense_task.await.unwrap();
    schedule_task.abort();

    let windows = *shell.windows.lock().unwrap();
    assert!(windows >= 50, "每句至少一个窗口：{windows}");
    assert!(
        *shell.ready.lock().unwrap() > 0,
        "应有句子走到日程 / 待办抽取，否则没覆盖这条路径"
    );

    // NFR-PRI-03：Q-STATE 请求里零文本
    let judged = ai.judged.lock().unwrap();
    let state_reqs: Vec<_> = judged
        .iter()
        .filter(|r| r.questions.contains(&Question::State))
        .collect();
    assert!(state_reqs.len() >= 50, "{}", state_reqs.len());
    for r in &state_reqs {
        let body = serde_json::to_string(&r.filtered()).unwrap();
        assert!(
            !body.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
            "Q-STATE 请求带了汉字：{body}"
        );
    }

    // NFR-PRI-04：全库零原文（整句和 6 字片段都查）
    drop(shell);
    let conn = rusqlite::Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let hits = grep(&conn, &sentences, 6, CONFIRMED_USER_TEXT).unwrap();
    assert!(hits.is_empty(), "数据库里搜到了原文：{hits:?}");
    // 对照：不放过日程 / 待办标题时能搜到片段（且只在这几列），说明抽取路径走到了、扫描本身有效
    let strict = grep(&conn, &sentences, 6, &[]).unwrap();
    assert!(!strict.is_empty());
    assert!(
        strict
            .iter()
            .all(|h| CONFIRMED_USER_TEXT.contains(&format!("{}.{}", h.table, h.column).as_str())),
        "{strict:?}"
    );
    drop(conn);
    let _ = std::fs::remove_dir_all(&dir);
}
