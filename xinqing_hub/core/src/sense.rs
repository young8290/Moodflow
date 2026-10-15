//! 实时感知（03 第 2.2 节 `feature_pipeline` 任务）：把 XQP 客户端收到的事件喂给状态识别流水线，
//! 更新显示状态、写库、发布总线事件，并把显示状态下发给输入法（工具栏天气按钮，FR-ENT-02）。
//!
//! 与外壳之间只通过 [`SensePort`]（状态快照、打开窗口、写库、日志），本模块不依赖 Tauri（ADR 0007）。
//!
//! 时间：流水线用核心的会话时间（毫秒）。收到新会话的第一条带 `ts` 的事件时，用“现在 − ts”
//! 定出会话起点对应的本地时间；之后每 250 ms 的 tick 用“最近一条事件的 ts + 收到它以来经过的时间”
//! 推算当前会话时间（17 第 2.3 节）。
//!
//! Jev 判断（FR-STA-05）：窗口结束时，若外壳给了 AI 网关且用户同意了 ②（`jev_features`），把白名单内的
//! 数值特征发给 Jev 问 Q-STATE / Q-VALENCE / Q-COMFORT，拿到结果（或失败、超时）后再融合。判断在后台任务里跑，
//! 不阻塞事件处理；同一时刻只有一个请求在途，期间结束的窗口排队，按顺序融合。没有网关、没有同意、
//! 请求失败或超过 [`JEV_DEADLINE`] 时，这个窗口只用本地规则（FR-STA-08）。

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast, mpsc};
use tokio::time::{Instant, MissedTickBehavior};
use xqp::{ByeReason, Down, MoodState, OpenTarget, Scope, Up};

use crate::bus::{HubEvent, MoodEvent};
use crate::domain::explain::{self, Evidence, ExplainSource, Explanation};
use crate::domain::features::baseline::next_recompute_after;
use crate::domain::features::{BaselineStats, WindowFeatures};
use crate::domain::fusion::{FusionOut, JevVerdict, Judged, Source};
use crate::domain::rules::Hints;
use crate::domain::self_report::{self, SelfWeather};
use crate::domain::status::StatusSnapshot;
use crate::infra::clock::Clock;
use crate::infra::gateway::{
    AiGateway, Answer, JudgeRequest, JudgeResponse, Question, STATE_FIELDS,
};
use crate::infra::xqp::{LinkEvent, XqpHandle};
use crate::pipeline::{PipelineOut, StatePipeline, WindowOut};

/// tick 间隔（17 第 2.3 节）。
pub const TICK: Duration = Duration::from_millis(250);

/// 等一个窗口的 Jev 判断最多这么久，超过按降级处理。网关自己的超时是 5 秒、可重试 1 次（FR-AIG-02），
/// 这里再兜一层，免得网关卡住时窗口一直不融合。
pub const JEV_DEADLINE: Duration = Duration::from_secs(12);

/// 外壳提供给感知任务的能力。
pub trait SensePort: Send + Sync {
    /// 修改状态快照；`f` 返回是否有变化，有变化时由外壳推送 `status:changed`。
    fn update_status(&self, f: &mut dyn FnMut(&mut StatusSnapshot) -> bool);
    /// 核心请求打开 Hub 的某个窗口（语言栏菜单、工具栏按钮，FR-ENT-01/02）。
    fn open(&self, target: OpenTarget);
    /// 保存一个特征窗口及其融合结果（D-06、D-07）。
    fn save_window(&self, rec: &WindowRecord<'_>);
    /// 运行日志。只会传入连接状态、计数这类不含用户数据的内容（NFR-LOG）。
    fn note(&self, msg: &str);
    /// 输入法总开关关闭时通知外壳退出 Hub。
    fn request_exit(&self) {}
    /// 显示状态切换了，附上本次的状态解释（FR-STA-09），外壳缓存到下一次切换，供 `state_explain` 返回。
    fn explained(&self, _e: &Explanation) {}
    /// 到了每天 04:00：用最近 7 天的窗口重算基线并写库（FR-STA-03 第 4 条），返回结果；失败时返回 `None`，
    /// 继续用现有基线。
    fn recompute_baseline(&self, _now_ms: i64) -> Option<BaselineStats> {
        None
    }
    /// 用户是否同意了 ②（把打字节奏特征发给 Jev，FR-ONB-04）。每个窗口结束时问一次，撤回立即生效。
    fn jev_allowed(&self) -> bool {
        false
    }
}

/// 后台判断任务交回的结果；`judged` 为 `None` 表示失败或超时。
#[derive(Debug)]
pub struct JevDone {
    seq: u64,
    judged: Option<Judged>,
}

/// 一个窗口的 Q-STATE / Q-VALENCE / Q-COMFORT 请求：FR-STA-02 的数值特征、应用类别和本地提示，
/// 不含任何文字和进程名（FR-STA-05、FR-AIG-05；网关发送前还会按白名单再过滤一次）。
pub fn state_request(w: &WindowOut) -> JudgeRequest {
    let mut state = match serde_json::to_value(&w.features) {
        Ok(serde_json::Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    };
    state.retain(|k, _| STATE_FIELDS.contains(&k.as_str()));
    state.insert("app_cat".into(), w.app_cat.as_str().into());
    state.insert("local_hints".into(), w.hints.joined().into());
    JudgeRequest {
        questions: vec![Question::State, Question::Valence, Question::Comfort],
        state,
    }
}

fn unit(p: f64) -> Option<f64> {
    p.is_finite().then(|| p.clamp(0.0, 1.0))
}

/// 读 Jev 的回答。Q-STATE 缺失、选项不是已知状态或概率不是数时整个结果作废（按降级处理）；
/// Q-VALENCE（0–4）、Q-COMFORT（0–1）缺了或不合法只把对应的值置空。
pub fn read_judged(resp: &JudgeResponse) -> Option<Judged> {
    let parse = |s: &str| serde_json::from_value::<MoodState>(s.into()).ok();
    let Some(Answer::Choice { probs, choice }) = resp.answers.get(&Question::State) else {
        return None;
    };
    let choice = parse(choice).filter(|s| *s != MoodState::Unknown)?;
    let probs: std::collections::HashMap<_, _> = probs
        .iter()
        .filter_map(|(k, p)| Some((parse(k)?, unit(*p)?)))
        .collect();
    let confidence = *probs.get(&choice)?;
    let valence = match resp.answers.get(&Question::Valence) {
        Some(Answer::Score { score }) if score.is_finite() => Some(score.clamp(0.0, 4.0)),
        _ => None,
    };
    let need_comfort = match resp.answers.get(&Question::Comfort) {
        Some(Answer::Noul { p }) => unit(*p),
        _ => None,
    };
    Some(Judged {
        verdict: JevVerdict {
            probs,
            choice,
            confidence,
            valence: valence.unwrap_or(2.0),
            need_comfort: need_comfort.unwrap_or(0.0),
        },
        valence,
        need_comfort,
        model: resp.model.clone(),
        latency_ms: resp.latency_ms,
    })
}

/// 一个已结束的窗口，时间已换算成 Unix 毫秒。
#[derive(Debug)]
pub struct WindowRecord<'a> {
    pub start_ms: i64,
    pub end_ms: i64,
    pub window: &'a WindowOut,
    pub fusion: &'a FusionOut,
    /// 这个窗口的 Jev 判断；降级时为 `None`。
    pub judged: Option<&'a Judged>,
}

/// 外壳发给感知任务的命令。
#[derive(Debug, Clone, PartialEq)]
pub enum SenseCmd {
    /// 用户在 Hub 里暂停 / 恢复感知（FR-WGT-06 右键菜单），同时经 XQP 下发给核心。
    Pause(bool),
    /// 用户对某个显示状态点了“不准”（FR-STA-07），`ts` 为 Unix 毫秒；已由外壳写入 `feedback` 表。
    Unfit { state: MoodState, ts: i64 },
    /// 用户自评（FR-STA-10），已由外壳写入 `self_report` 表。`raise` 是连续差异满 3 次时要上调阈值的自动状态。
    SelfReport {
        weather: SelfWeather,
        ts: i64,
        raise: Option<MoodState>,
    },
    /// 换上新的个人基线（用户“重置基线”后，外壳已写库，FR-STA-03 第 4 条）。
    Baseline(BaselineStats),
    /// 专注计时开始 / 结束（FR-RST-10）：期间自动判断照常记录，但不切换小组件天气；结束后显示回到自动判断。
    Focus(bool),
}

pub struct Sense {
    pipeline: StatePipeline,
    port: Arc<dyn SensePort>,
    xqp: XqpHandle,
    bus: broadcast::Sender<HubEvent>,
    clock: Arc<dyn Clock>,
    /// 当前核心会话（握手 `hello.session`）。
    session: Option<String>,
    /// 新会话还没收到带 `ts` 的事件，会话起点未定。
    needs_base: bool,
    /// 最近一条带 `ts` 的事件及收到它的时刻，用来推算当前会话时间。
    anchor: Option<(u64, Instant)>,
    /// 用户开启了无痕（Hub 右键菜单或输入法菜单、快捷键）。
    user_paused: bool,
    /// 当前输入框是密码框、禁用输入或黑名单应用（FR-SEN-05），小组件显示“闭眼”。
    gate_closed: bool,
    /// 上一个窗口的特征和规则提示，状态解释要看“触发窗口和上一个窗口”（FR-STA-09）。
    prev_window: Option<(WindowFeatures, Hints)>,
    /// 最近一次切换的状态解释，缓存到下一次切换。
    explanation: Option<Explanation>,
    /// 自评的显示覆盖：(用户说的状态, 到期 Unix 毫秒)。期间自动判断照常运行，但不改显示。
    self_shown: Option<(MoodState, i64)>,
    /// 下一次基线重算的时刻（每天 04:00）。
    next_recompute: chrono::DateTime<chrono::Local>,
    /// 专注计时中：显示冻结在开始时的状态
    focusing: bool,
    /// Jev（FR-STA-05）；外壳没给时只用本地规则。
    gateway: Option<Arc<dyn AiGateway>>,
    jev_tx: mpsc::UnboundedSender<JevDone>,
    jev_rx: Option<mpsc::UnboundedReceiver<JevDone>>,
    /// 等 Jev 判断的窗口，按结束顺序；队首在途时 `in_flight` 是它的序号。
    pending: VecDeque<WindowOut>,
    in_flight: Option<u64>,
    next_seq: u64,
}

impl Sense {
    pub fn new(
        pipeline: StatePipeline,
        port: Arc<dyn SensePort>,
        xqp: XqpHandle,
        bus: broadcast::Sender<HubEvent>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let next_recompute = next_recompute_after(clock.now());
        let (jev_tx, jev_rx) = mpsc::unbounded_channel();
        Self {
            pipeline,
            port,
            xqp,
            bus,
            clock,
            session: None,
            needs_base: true,
            anchor: None,
            user_paused: false,
            gate_closed: false,
            prev_window: None,
            explanation: None,
            self_shown: None,
            focusing: false,
            next_recompute,
            gateway: None,
            jev_tx,
            jev_rx: Some(jev_rx),
            pending: VecDeque::new(),
            in_flight: None,
            next_seq: 0,
        }
    }

    /// 接上 AI 网关：之后结束的窗口在用户同意 ② 时先问 Jev 再融合。
    pub fn with_gateway(mut self, gateway: Arc<dyn AiGateway>) -> Self {
        self.gateway = Some(gateway);
        self
    }

    /// 最近一次自动判断切换的解释（自评期间也是自动判断的）；还没切换过时为 `None`。
    pub fn explanation(&self) -> Option<&Explanation> {
        self.explanation.as_ref()
    }

    /// 运行到 `cmds` 的发送端全部丢弃为止。`link` 关闭（没有连接核心）后只处理命令。
    pub async fn run(
        mut self,
        mut link: mpsc::Receiver<LinkEvent>,
        mut cmds: mpsc::Receiver<SenseCmd>,
    ) {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut link_open = true;
        let mut jev = self.jev_rx.take().expect("run 只调用一次");
        loop {
            tokio::select! {
                Some(done) = jev.recv() => self.on_jev(done),
                ev = link.recv(), if link_open => match ev {
                    Some(ev) => self.on_link(ev, Instant::now()),
                    None => link_open = false,
                },
                cmd = cmds.recv() => match cmd {
                    Some(c) => self.on_cmd(c),
                    None => return,
                },
                now = tick.tick() => self.on_tick(now),
            }
        }
    }

    pub fn on_link(&mut self, ev: LinkEvent, now: Instant) {
        match ev {
            LinkEvent::Connected(peer) => {
                if self.session.as_deref() == Some(peer.session.as_str()) {
                    // 同一会话重连：断开期间核心没发事件，进行中的窗口不完整
                    self.pipeline.discard();
                } else {
                    self.session = Some(peer.session.clone());
                    self.needs_base = true;
                    self.anchor = None;
                    self.prev_window = None;
                    self.pipeline.reset_session(self.clock.now());
                }
                self.pipeline.push(&peer.to_hello());
                self.gate_closed = false;
                self.port
                    .note(&format!("XQP 已连接（核心 {}）", peer.ime_ver));
                let paused = self.paused();
                self.port.update_status(&mut |s| {
                    StatusSnapshot::set(&mut s.connected, true)
                        | StatusSnapshot::set(&mut s.paused, paused)
                });
            }
            LinkEvent::Disconnected(d) => {
                self.pipeline.discard();
                self.gate_closed = false;
                self.port.note(&format!("XQP 已断开（{}）", d.label()));
                let paused = self.paused();
                self.port.update_status(&mut |s| {
                    StatusSnapshot::set(&mut s.connected, false)
                        | StatusSnapshot::set(&mut s.paused, paused)
                });
                if matches!(d, crate::infra::xqp::Disconnect::Bye(ByeReason::Disabled)) {
                    self.port.request_exit();
                }
            }
            LinkEvent::Up(up) => self.on_up(up, now),
        }
    }

    pub fn on_cmd(&mut self, cmd: SenseCmd) {
        match cmd {
            SenseCmd::Pause(on) => {
                self.apply_pause(on);
                self.xqp.send(Down::Pause { on });
            }
            SenseCmd::Unfit { state, ts } => self.pipeline.fusion_mut().record_unfit(state, ts),
            SenseCmd::SelfReport { weather, ts, raise } => self.on_self_report(weather, ts, raise),
            SenseCmd::Baseline(stats) => self.apply_baseline(&stats),
            SenseCmd::Focus(on) => {
                let was = std::mem::replace(&mut self.focusing, on);
                // 结束时显示回到自动判断（自评覆盖还在时仍以自评为准）
                if was && !on && self.self_shown.is_none() {
                    if let Some(e) = &self.explanation {
                        self.port.explained(e);
                    }
                    let auto = self.pipeline.fusion_mut().shown();
                    self.show(auto);
                }
            }
        }
    }

    pub fn on_tick(&mut self, now: Instant) {
        self.expire_self_report();
        self.maybe_recompute_baseline();
        if self.user_paused {
            return;
        }
        let Some((ts, at)) = self.anchor else {
            return;
        };
        let now_ts = ts + now.saturating_duration_since(at).as_millis() as u64;
        let outs = self.pipeline.tick(now_ts);
        self.handle(outs);
    }

    fn maybe_recompute_baseline(&mut self) {
        let now = self.clock.now();
        if now < self.next_recompute {
            return;
        }
        self.next_recompute = next_recompute_after(now);
        if let Some(stats) = self.port.recompute_baseline(now.timestamp_millis()) {
            self.port.note(&format!(
                "基线已重算：{} 个窗口，{} 项个人统计",
                stats.windows,
                stats.rows.len()
            ));
            self.apply_baseline(&stats);
        }
    }

    fn apply_baseline(&mut self, stats: &BaselineStats) {
        self.pipeline.baseline_mut().apply(stats);
        let progress = self.pipeline.baseline().progress_pct();
        self.port
            .update_status(&mut |s| StatusSnapshot::set(&mut s.baseline_progress, progress));
    }

    /// 自评（FR-STA-10）：之后 60 分钟显示用户说的状态；“说不上来”结束之前的覆盖、回到自动判断。
    fn on_self_report(&mut self, weather: SelfWeather, ts: i64, raise: Option<MoodState>) {
        if let Some(s) = raise {
            self.pipeline.fusion_mut().raise(s, ts);
        }
        let until = match weather.state() {
            Some(state) => {
                let until = ts + self_report::OVERRIDE_MS;
                self.self_shown = Some((state, until));
                self.port.explained(&Explanation::self_report(state));
                self.show(state);
                until
            }
            None => {
                self.end_self_report();
                ts
            }
        };
        let _ = self.bus.send(HubEvent::SelfReport { weather, until });
    }

    fn self_report_active(&self) -> bool {
        let now = self.clock.now().timestamp_millis();
        self.self_shown.is_some_and(|(_, until)| now < until)
    }

    fn expire_self_report(&mut self) {
        if self.self_shown.is_some() && !self.self_report_active() {
            self.end_self_report();
        }
    }

    /// 结束自评覆盖，显示回到自动判断及其解释。
    fn end_self_report(&mut self) {
        if self.self_shown.take().is_none() {
            return;
        }
        if let Some(e) = &self.explanation {
            self.port.explained(e);
        }
        let auto = self.pipeline.fusion_mut().shown();
        self.show(auto);
    }

    /// 更新显示状态，变化时下发给输入法（工具栏天气按钮，FR-ENT-02）。
    fn show(&mut self, state: MoodState) {
        let mut offline = true;
        let mut changed = false;
        self.port.update_status(&mut |s| {
            offline = s.offline;
            changed = s.state != state;
            s.apply_mood(state, None)
        });
        if changed {
            self.xqp.send(Down::Mood { state, offline });
        }
    }

    fn paused(&self) -> bool {
        self.user_paused || self.gate_closed
    }

    fn refresh_paused(&self) {
        let paused = self.paused();
        self.port
            .update_status(&mut |s| StatusSnapshot::set(&mut s.paused, paused));
    }

    fn apply_pause(&mut self, on: bool) {
        self.user_paused = on;
        if on {
            self.pipeline.discard();
        }
        let _ = self.bus.send(HubEvent::Pause(on));
        self.refresh_paused();
    }

    fn on_up(&mut self, up: Up, now: Instant) {
        if let Some(ts) = up.ts() {
            if self.needs_base {
                let base = self.clock.now() - chrono::Duration::milliseconds(ts as i64);
                self.pipeline.set_base_local(base);
                self.needs_base = false;
            }
            self.anchor = Some((ts, now));
        }
        match &up {
            Up::Focus { scope, blocked, .. } => {
                self.gate_closed = *blocked || *scope != Scope::Normal;
                self.refresh_paused();
            }
            Up::PauseChanged { on, .. } => {
                self.apply_pause(*on);
                // 核心已经切换了，回发一次只是让重连后的补发状态与它一致
                self.xqp.send(Down::Pause { on: *on });
            }
            Up::Open { target, .. } => self.port.open(*target),
            Up::Hb { dropped, .. } if *dropped > 0 => {
                self.port
                    .note(&format!("核心队列满，丢弃了 {dropped} 条事件（FR-SEN-07）"));
            }
            _ => {}
        }
        let up = Arc::new(up);
        let _ = self.bus.send(HubEvent::Xqp(up.clone()));
        if self.user_paused {
            // 无痕期间核心不发采集事件；万一收到也不计算（FR-SEN-06）
            return;
        }
        let outs = self.pipeline.push(&up);
        self.handle(outs);
    }

    fn handle(&mut self, outs: Vec<PipelineOut>) {
        for o in outs {
            match o {
                PipelineOut::Typo { ts } => {
                    let ts = self.pipeline.unix_ms(ts);
                    let _ = self.bus.send(HubEvent::Typo { ts });
                }
                PipelineOut::Window(w) => self.on_window(*w),
            }
        }
    }

    fn on_window(&mut self, w: WindowOut) {
        if self.pending.is_empty() && !self.jev_enabled() {
            self.finish_window(w, None);
            return;
        }
        self.pending.push_back(w);
        self.pump();
    }

    fn jev_enabled(&self) -> bool {
        self.gateway.is_some() && self.port.jev_allowed()
    }

    /// 队首没有在途请求时：能问 Jev 就发出去，否则（撤回了 ②）直接按规则融合，再看下一个。
    fn pump(&mut self) {
        while self.in_flight.is_none() {
            let Some(w) = self.pending.front() else {
                return;
            };
            match self.gateway.clone().filter(|_| self.port.jev_allowed()) {
                Some(gw) => {
                    let seq = self.next_seq;
                    self.next_seq += 1;
                    self.in_flight = Some(seq);
                    let req = state_request(w);
                    let tx = self.jev_tx.clone();
                    tokio::spawn(async move {
                        let judged = match tokio::time::timeout(JEV_DEADLINE, gw.judge(req)).await {
                            Ok(Ok(resp)) => read_judged(&resp),
                            _ => None,
                        };
                        let _ = tx.send(JevDone { seq, judged });
                    });
                }
                None => {
                    let w = self.pending.pop_front().expect("队首存在");
                    self.finish_window(w, None);
                }
            }
        }
    }

    /// Jev 判断回来了（或失败、超时）：融合队首窗口，再处理排在后面的。
    pub fn on_jev(&mut self, done: JevDone) {
        if self.in_flight != Some(done.seq) {
            return;
        }
        self.in_flight = None;
        if let Some(w) = self.pending.pop_front() {
            self.finish_window(w, done.judged);
        }
        self.pump();
    }

    fn finish_window(&mut self, w: WindowOut, judged: Option<Judged>) {
        let fusion = self.pipeline.fuse(&w, judged.as_ref().map(|j| &j.verdict));
        let rec = WindowRecord {
            start_ms: self.pipeline.unix_ms(w.start_ts),
            end_ms: self.pipeline.unix_ms(w.end_ts),
            window: &w,
            fusion: &fusion,
            judged: judged.as_ref(),
        };
        self.port.save_window(&rec);
        let ts = rec.end_ms;
        let _ = self.bus.send(HubEvent::Window(Arc::new((
            w.features.clone(),
            w.hints.clone(),
        ))));
        let _ = self.bus.send(HubEvent::Mood(MoodEvent::Sample {
            ts,
            out: fusion.clone(),
            // 降级运行时没有，暖心话不主动关怀（FR-STA-08 第 2 条）
            need_comfort: judged.as_ref().and_then(|j| j.need_comfort),
            valence: judged.as_ref().and_then(|j| j.valence),
        }));
        if fusion.changed {
            // 只有本地规则时界面不显示百分比
            let (source, prob) = match fusion.source {
                Source::Jev => (
                    ExplainSource::Jev,
                    judged.as_ref().map(|j| j.verdict.p(fusion.shown)),
                ),
                Source::Rule => (ExplainSource::Rule, None),
            };
            let e = explain::build(
                fusion.shown,
                prob,
                source,
                Evidence {
                    features: &w.features,
                    hints: &w.hints,
                },
                self.prev_window
                    .as_ref()
                    .map(|(features, hints)| Evidence { features, hints }),
                self.pipeline.baseline(),
            );
            self.explanation = Some(e);
        }
        self.expire_self_report();
        // 自评覆盖或专注中：自动判断照常运行，但不改显示
        let self_active = self.self_shown.is_some() || self.focusing;
        // 先交出解释再推送状态：界面收到 status:changed 后取到的一定是同一次切换的解释
        if fusion.changed
            && !self_active
            && let Some(e) = &self.explanation
        {
            self.port.explained(e);
        }
        let progress = self.pipeline.baseline().progress_pct();
        let shown = (!self_active).then_some(fusion.shown);
        let mut offline = true;
        let mut moved = false;
        self.port.update_status(&mut |s| {
            offline = s.offline;
            let mut changed = StatusSnapshot::set(&mut s.baseline_progress, progress);
            if let Some(state) = shown {
                moved = s.state != state;
                changed |= s.apply_mood(state, None);
            }
            changed
        });
        if moved {
            self.xqp.send(Down::Mood {
                state: fusion.shown,
                offline,
            });
        }
        if fusion.changed {
            let _ = self.bus.send(HubEvent::Mood(MoodEvent::StateChanged {
                ts,
                state: fusion.shown,
            }));
        }
        self.prev_window = Some((w.features, w.hints));
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Mutex;

    use chrono::{Local, TimeZone};
    use xqp::{ByeReason, KeyKind, KeySrc};

    use super::*;
    use crate::domain::features::Baseline;
    use crate::infra::clock::ManualClock;
    use crate::infra::templates::{AppCategories, BaselineDefault, TemplateDirs};
    use crate::infra::xqp::{Disconnect, PeerInfo};

    #[derive(Default)]
    struct FakePort {
        status: Mutex<StatusSnapshot>,
        pushes: Mutex<u32>,
        opened: Mutex<Vec<OpenTarget>>,
        saved: Mutex<Vec<(i64, i64)>>,
        notes: Mutex<Vec<String>>,
        explained: Mutex<Vec<Explanation>>,
        recompute: Mutex<Option<BaselineStats>>,
        exit_requests: Mutex<u32>,
        jev: std::sync::atomic::AtomicBool,
    }

    impl SensePort for FakePort {
        fn update_status(&self, f: &mut dyn FnMut(&mut StatusSnapshot) -> bool) {
            if f(&mut self.status.lock().unwrap()) {
                *self.pushes.lock().unwrap() += 1;
            }
        }
        fn open(&self, target: OpenTarget) {
            self.opened.lock().unwrap().push(target);
        }
        fn save_window(&self, rec: &WindowRecord<'_>) {
            self.saved.lock().unwrap().push((rec.start_ms, rec.end_ms));
        }
        fn note(&self, msg: &str) {
            self.notes.lock().unwrap().push(msg.to_string());
        }
        fn request_exit(&self) {
            *self.exit_requests.lock().unwrap() += 1;
        }
        fn explained(&self, e: &Explanation) {
            self.explained.lock().unwrap().push(e.clone());
        }
        fn recompute_baseline(&self, _now_ms: i64) -> Option<BaselineStats> {
            self.recompute.lock().unwrap().clone()
        }
        fn jev_allowed(&self) -> bool {
            self.jev.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    struct Rig {
        sense: Sense,
        port: Arc<FakePort>,
        down: mpsc::Receiver<Down>,
        bus: broadcast::Receiver<HubEvent>,
        clock: Arc<ManualClock>,
        t0: Instant,
    }

    fn rig() -> Rig {
        let dirs = TemplateDirs::factory_only(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../hub_templates"),
        );
        let base = Baseline::from_defaults(&BaselineDefault::load(&dirs).unwrap());
        let start = Local.with_ymd_and_hms(2026, 10, 5, 14, 0, 0).unwrap();
        let pipeline = StatePipeline::new(base, AppCategories::load(&dirs).unwrap(), start);
        let port = Arc::new(FakePort::default());
        let (xqp, down) = XqpHandle::pair();
        let (bus_tx, bus) = crate::bus::channel();
        let clock = Arc::new(ManualClock::new(start));
        let sense = Sense::new(pipeline, port.clone(), xqp, bus_tx, clock.clone());
        Rig {
            sense,
            port,
            down,
            bus,
            clock,
            t0: Instant::now(),
        }
    }

    fn connected(session: &str) -> LinkEvent {
        LinkEvent::Connected(PeerInfo {
            ime_ver: "test".into(),
            session: session.into(),
            caps: vec!["core_keys".into()],
        })
    }

    fn key(ts: u64) -> Up {
        Up::Key {
            ts,
            seq: None,
            kind: KeyKind::Letter,
            vk: Some(b'A'),
            in_comp: false,
            src: KeySrc::Core,
            eaten: None,
            del_committed: None,
        }
    }

    fn focus(ts: u64, scope: Scope) -> Up {
        Up::Focus {
            ts,
            seq: None,
            app: None,
            scope,
            blocked: false,
        }
    }

    fn downs(rx: &mut mpsc::Receiver<Down>) -> Vec<Down> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    impl Rig {
        fn up(&mut self, ev: Up) {
            let at = self.t0 + Duration::from_millis(ev.ts().unwrap_or(0));
            self.sense.on_link(LinkEvent::Up(ev), at);
        }
        fn status(&self) -> StatusSnapshot {
            self.port.status.lock().unwrap().clone()
        }
        /// 按真实节奏回放一个合成脚本，返回其间自动判断的切换序列（StateChanged）。
        fn play(&mut self, name: &str) -> Vec<MoodState> {
            let text = std::fs::read_to_string(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join(format!("../../tools/xq-sim/scripts/{name}.jsonl")),
            )
            .unwrap();
            let mut changes = Vec::new();
            // 边回放边读总线，脚本事件多于总线容量
            let mut drain = |bus: &mut broadcast::Receiver<HubEvent>| {
                while let Ok(ev) = bus.try_recv() {
                    if let HubEvent::Mood(MoodEvent::StateChanged { state, .. }) = ev {
                        changes.push(state);
                    }
                }
            };
            let mut last = 0;
            for line in text.lines() {
                let ev: Up = serde_json::from_str(line).unwrap();
                if let Some(ts) = ev.ts() {
                    for t in (last..ts).step_by(250).skip(1) {
                        self.sense.on_tick(self.t0 + Duration::from_millis(t));
                    }
                    last = ts;
                }
                self.up(ev);
                drain(&mut self.bus);
            }
            self.sense
                .on_tick(self.t0 + Duration::from_millis(last + 5_000));
            drain(&mut self.bus);
            changes
        }
    }

    #[test]
    fn connection_state_reaches_the_status() {
        let mut r = rig();
        r.sense.on_link(connected("s1"), r.t0);
        assert!(r.status().connected);
        r.sense
            .on_link(LinkEvent::Disconnected(Disconnect::Idle), r.t0);
        assert!(!r.status().connected);
        let notes = r.port.notes.lock().unwrap();
        assert!(notes.iter().any(|n| n.contains("idle")));
    }

    #[test]
    fn disabled_bye_requests_hub_exit_but_shutdown_does_not() {
        let mut r = rig();
        r.sense.on_link(
            LinkEvent::Disconnected(Disconnect::Bye(ByeReason::Shutdown)),
            r.t0,
        );
        assert_eq!(*r.port.exit_requests.lock().unwrap(), 0);

        r.sense.on_link(
            LinkEvent::Disconnected(Disconnect::Bye(ByeReason::Disabled)),
            r.t0,
        );
        assert_eq!(*r.port.exit_requests.lock().unwrap(), 1);
    }

    #[test]
    fn typing_produces_windows_with_unix_times() {
        let mut r = rig();
        r.sense.on_link(connected("s1"), r.t0);
        for i in 0..12 {
            r.up(key(1_000 + i * 150));
        }
        // 停顿超过 2 秒后由 tick 结束窗口
        let last = 1_000 + 11 * 150;
        r.sense.on_tick(r.t0 + Duration::from_millis(last + 2_100));
        let saved = r.port.saved.lock().unwrap().clone();
        assert_eq!(saved.len(), 1);
        // 会话起点 = 收到第一条事件时的“现在” − 它的 ts
        let base = Local
            .with_ymd_and_hms(2026, 10, 5, 14, 0, 0)
            .unwrap()
            .timestamp_millis()
            - 1_000;
        assert_eq!(saved[0], (base + 1_000, base + last as i64));
        let mut kinds = Vec::new();
        while let Ok(ev) = r.bus.try_recv() {
            kinds.push(match ev {
                HubEvent::Xqp(_) => "xqp",
                HubEvent::Window(_) => "window",
                HubEvent::Mood(MoodEvent::Sample { .. }) => "sample",
                _ => "other",
            });
        }
        assert!(kinds.contains(&"window") && kinds.contains(&"sample"));
    }

    #[test]
    fn state_change_comes_with_explanation() {
        // 回放合成的犹豫脚本（只有本地规则），每次切换都带解释（FR-STA-09、KPI-10）
        let mut r = rig();
        r.sense.on_link(connected("s1"), r.t0);
        let changes = r.play("hesitant");

        let explained = r.port.explained.lock().unwrap().clone();
        assert!(changes.contains(&MoodState::Hesitant), "{changes:?}");
        assert_eq!(
            explained.iter().map(|e| e.state).collect::<Vec<_>>(),
            changes
        );
        let hesitant = explained
            .iter()
            .find(|e| e.state == MoodState::Hesitant)
            .unwrap();
        assert!(!hesitant.signals.is_empty());
        assert_eq!(hesitant.source, ExplainSource::Rule);
        assert_eq!(hesitant.prob_pct, None);
        assert_eq!(r.sense.explanation(), explained.last());
    }

    #[test]
    fn self_report_overrides_display_for_an_hour() {
        // FR-STA-10：自评后 60 分钟显示用户说的状态，自动判断照常在后台运行
        let mut r = rig();
        r.sense.on_link(connected("s1"), r.t0);
        let now = r.clock.now().timestamp_millis();
        r.sense.on_cmd(SenseCmd::SelfReport {
            weather: SelfWeather::Night,
            ts: now,
            raise: Some(MoodState::Hesitant),
        });
        assert_eq!(r.status().state, MoodState::Tired);
        assert_eq!(
            r.port.explained.lock().unwrap().last(),
            Some(&Explanation::self_report(MoodState::Tired))
        );
        assert!(downs(&mut r.down).contains(&Down::Mood {
            state: MoodState::Tired,
            offline: true
        }));
        let mut until = None;
        while let Ok(ev) = r.bus.try_recv() {
            if let HubEvent::SelfReport { weather, until: u } = ev {
                assert_eq!(weather, SelfWeather::Night);
                until = Some(u);
            }
        }
        assert_eq!(until, Some(now + self_report::OVERRIDE_MS));
        // 连续差异满 3 次：被否定的自动状态阈值上调
        assert!(r.sense.pipeline.fusion_mut().bump(MoodState::Hesitant, now) > 0.0);

        // 覆盖期间自动判断切到犹豫又回来，显示不变、解释不被覆盖
        let auto = r.play("hesitant");
        assert!(auto.contains(&MoodState::Hesitant));
        assert_eq!(r.status().state, MoodState::Tired);
        assert_eq!(
            r.port.explained.lock().unwrap().last().map(|e| e.source),
            Some(ExplainSource::SelfReport)
        );

        // 60 分钟后回到自动判断，并交出自动判断的解释
        r.clock.advance_ms(self_report::OVERRIDE_MS + 1);
        r.sense.on_tick(r.t0 + Duration::from_secs(600));
        assert_eq!(r.status().state, r.sense.pipeline.fusion_mut().shown());
        assert_eq!(
            r.port.explained.lock().unwrap().last(),
            r.sense.explanation()
        );
    }

    #[test]
    fn unsure_ends_the_override() {
        let mut r = rig();
        let now = r.clock.now().timestamp_millis();
        r.sense.on_cmd(SenseCmd::SelfReport {
            weather: SelfWeather::Storm,
            ts: now,
            raise: None,
        });
        assert_eq!(r.status().state, MoodState::Agitated);
        r.sense.on_cmd(SenseCmd::SelfReport {
            weather: SelfWeather::Unsure,
            ts: now + 1_000,
            raise: None,
        });
        assert_eq!(r.status().state, MoodState::Fluent);
        // “说不上来”本身不覆盖显示
        r.sense.on_cmd(SenseCmd::SelfReport {
            weather: SelfWeather::Unsure,
            ts: now + 2_000,
            raise: None,
        });
        assert_eq!(r.status().state, MoodState::Fluent);
    }

    #[test]
    fn baseline_recomputes_at_four_am_and_on_reset() {
        let mut r = rig();
        let stats = BaselineStats {
            rows: Vec::new(),
            windows: 150,
        };
        *r.port.recompute.lock().unwrap() = Some(stats.clone());
        // rig 的时钟在 14:00，没到 04:00 不重算
        r.sense.on_tick(r.t0);
        assert_eq!(r.status().baseline_progress, 0);
        r.clock.advance_ms(14 * 3_600_000);
        r.sense.on_tick(r.t0);
        assert_eq!(r.status().baseline_progress, 75);
        assert_eq!(r.sense.pipeline.baseline().windows, 150);
        // 当天只重算一次
        *r.port.recompute.lock().unwrap() = Some(BaselineStats::default());
        r.sense.on_tick(r.t0);
        assert_eq!(r.sense.pipeline.baseline().windows, 150);
        // 重置基线：外壳写库后交来空结果，回到冷启动
        r.sense.on_cmd(SenseCmd::Baseline(BaselineStats::default()));
        assert_eq!(r.status().baseline_progress, 0);
        assert!(r.sense.pipeline.baseline().is_cold());
    }

    #[test]
    fn unfit_feedback_reaches_fusion() {
        // FR-STA-07：外壳写库后把“不准”交给感知任务，30 分钟内 3 次 → 阈值 +0.05
        let mut r = rig();
        for t in [0, 60_000, 120_000] {
            r.sense.on_cmd(SenseCmd::Unfit {
                state: MoodState::Low,
                ts: t,
            });
        }
        let f = r.sense.pipeline.fusion_mut();
        assert!((f.bump(MoodState::Low, 130_000) - 0.05).abs() < 1e-9);
        assert_eq!(f.bump(MoodState::Hesitant, 130_000), 0.0);
    }

    #[test]
    fn pause_from_hub_discards_window_and_is_sent_to_core() {
        let mut r = rig();
        r.sense.on_link(connected("s1"), r.t0);
        for i in 0..8 {
            r.up(key(100 + i * 150));
        }
        r.sense.on_cmd(SenseCmd::Pause(true));
        assert!(r.status().paused);
        assert_eq!(downs(&mut r.down), vec![Down::Pause { on: true }]);
        r.sense.on_tick(r.t0 + Duration::from_secs(10));
        assert!(r.port.saved.lock().unwrap().is_empty(), "无痕后窗口作废");
        r.sense.on_cmd(SenseCmd::Pause(false));
        assert!(!r.status().paused);
    }

    #[test]
    fn pause_from_core_updates_status_and_sticky_state() {
        let mut r = rig();
        r.sense.on_link(connected("s1"), r.t0);
        r.up(Up::PauseChanged {
            ts: 50,
            seq: None,
            on: true,
            by: xqp::PauseBy::Hotkey,
        });
        assert!(r.status().paused);
        assert_eq!(downs(&mut r.down), vec![Down::Pause { on: true }]);
    }

    #[test]
    fn password_field_shows_paused_until_focus_moves() {
        let mut r = rig();
        r.sense.on_link(connected("s1"), r.t0);
        r.up(focus(10, Scope::Password));
        assert!(r.status().paused);
        r.up(focus(20, Scope::Normal));
        assert!(!r.status().paused);
        r.up(Up::Focus {
            ts: 30,
            seq: None,
            app: None,
            scope: Scope::Normal,
            blocked: true,
        });
        assert!(r.status().paused);
        r.sense.on_link(
            LinkEvent::Disconnected(Disconnect::Bye(ByeReason::Shutdown)),
            r.t0,
        );
        assert!(!r.status().paused, "断开后不再显示闭眼");
    }

    #[test]
    fn open_requests_and_rewrite_left_to_the_rewrite_service() {
        let mut r = rig();
        r.sense.on_link(connected("s1"), r.t0);
        r.up(Up::Open {
            ts: 5,
            seq: None,
            target: OpenTarget::Chat,
        });
        assert_eq!(*r.port.opened.lock().unwrap(), vec![OpenTarget::Chat]);
        r.up(Up::RewriteReq {
            ts: 6,
            seq: None,
            req_id: 7,
            source: xqp::RewriteSource::Recent,
            text: "原文".into(),
            style: xqp::RewriteStyle::Gentle,
            replace_len: Some(2),
        });
        // 改写请求经总线交给改写服务（`crate::rewrite`），感知任务自己不回
        assert!(downs(&mut r.down).is_empty());
    }

    #[test]
    fn new_session_restarts_session_time() {
        let mut r = rig();
        r.sense.on_link(connected("s1"), r.t0);
        for i in 0..6 {
            r.up(key(500_000 + i * 150));
        }
        // 核心重启：会话时间从 0 开始，进行中的窗口作废，不会拿新旧时间拼成一个窗口
        r.sense.on_link(
            LinkEvent::Disconnected(Disconnect::Closed),
            r.t0 + Duration::from_secs(1),
        );
        r.sense
            .on_link(connected("s2"), r.t0 + Duration::from_secs(2));
        for i in 0..6 {
            r.up(key(100 + i * 150));
        }
        r.sense
            .on_tick(r.t0 + Duration::from_millis(100 + 5 * 150 + 2_100));
        let saved = r.port.saved.lock().unwrap().clone();
        assert_eq!(saved.len(), 1);
        assert!(saved[0].1 - saved[0].0 < 1_000);
    }

    #[test]
    fn focus_freezes_the_shown_weather_until_it_ends() {
        let mut r = rig();
        r.sense.on_link(connected("s1"), r.t0);
        r.sense.on_cmd(SenseCmd::Focus(true));
        let changes = r.play("hesitant");
        assert!(
            changes.contains(&MoodState::Hesitant),
            "自动判断照常：{changes:?}"
        );
        assert_eq!(r.status().state, MoodState::Fluent, "专注中不切换显示");
        r.sense.on_cmd(SenseCmd::Focus(false));
        assert_eq!(
            r.status().state,
            r.sense.pipeline.fusion_mut().shown(),
            "结束后回到自动判断"
        );
    }

    mod jev {
        use std::collections::HashMap;
        use std::sync::atomic::Ordering;

        use async_trait::async_trait;
        use futures::stream::BoxStream;

        use super::*;
        use crate::infra::gateway::{
            AiError, CompleteRequest, CompleteResponse, Delta, GatewayHealth,
        };

        /// 回固定答案（或固定错误）的 Jev，记下收到的请求。
        struct FakeJev {
            reply: Result<JudgeResponse, AiError>,
            reqs: Mutex<Vec<JudgeRequest>>,
        }

        #[async_trait]
        impl AiGateway for FakeJev {
            async fn judge(&self, req: JudgeRequest) -> Result<JudgeResponse, AiError> {
                self.reqs.lock().unwrap().push(req);
                self.reply.clone()
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
                    llm: true,
                }
            }
        }

        fn response(choice: &str, p: f64) -> JudgeResponse {
            let rest = (1.0 - p) / 4.0;
            let probs = ["fluent", "hesitant", "low", "agitated", "tired"]
                .into_iter()
                .map(|s| (s.to_string(), if s == choice { p } else { rest }))
                .collect();
            JudgeResponse {
                model: "jev-test".into(),
                latency_ms: 40,
                answers: HashMap::from([
                    (
                        Question::State,
                        Answer::Choice {
                            probs,
                            choice: choice.into(),
                        },
                    ),
                    (Question::Valence, Answer::Score { score: 1.0 }),
                    (Question::Comfort, Answer::Noul { p: 0.8 }),
                ]),
            }
        }

        fn jev_rig(reply: Result<JudgeResponse, AiError>, allowed: bool) -> (Rig, Arc<FakeJev>) {
            let mut r = rig();
            let gw = Arc::new(FakeJev {
                reply,
                reqs: Mutex::new(Vec::new()),
            });
            r.sense.gateway = Some(gw.clone());
            r.port.jev.store(allowed, Ordering::SeqCst);
            r.sense.on_link(connected("s1"), r.t0);
            (r, gw)
        }

        impl Rig {
            /// 从 `start` 起打 12 个键，停顿 2 秒以上结束一个窗口。
            fn type_window(&mut self, start: u64) {
                for i in 0..12 {
                    self.up(key(start + i * 150));
                }
                let last = start + 11 * 150;
                self.sense
                    .on_tick(self.t0 + Duration::from_millis(last + 2_100));
            }
            async fn jev_done(&mut self) {
                let done = self.sense.jev_rx.as_mut().unwrap().recv().await.unwrap();
                self.sense.on_jev(done);
            }
            fn samples(&mut self) -> Vec<(Option<f64>, Option<f64>, Source)> {
                std::iter::from_fn(|| self.bus.try_recv().ok())
                    .filter_map(|ev| match ev {
                        HubEvent::Mood(MoodEvent::Sample {
                            out,
                            need_comfort,
                            valence,
                            ..
                        }) => Some((need_comfort, valence, out.source)),
                        _ => None,
                    })
                    .collect()
            }
        }

        #[test]
        fn request_carries_only_whitelisted_numbers() {
            let w = WindowOut {
                start_ts: 0,
                end_ts: 1,
                app_cat: crate::infra::templates::AppCat::Chat,
                features: WindowFeatures {
                    kpm: Some(210.0),
                    minute_of_day: 600,
                    ..Default::default()
                },
                hints: Hints(vec![crate::domain::rules::Hint::HesitationHint]),
            };
            let req = state_request(&w);
            assert_eq!(
                req.questions,
                [Question::State, Question::Valence, Question::Comfort]
            );
            assert!(req.state.keys().all(|k| STATE_FIELDS.contains(&k.as_str())));
            assert!(!req.state.contains_key("minute_of_day"));
            assert_eq!(req.state["kpm"], 210.0);
            assert_eq!(req.state["app_cat"], "chat");
            assert_eq!(req.state["local_hints"], "hesitation_hint");
            assert_eq!(req.filtered(), req.state, "网关的白名单过滤不再删任何字段");
        }

        #[test]
        fn answers_are_validated() {
            let j = read_judged(&response("agitated", 0.95)).unwrap();
            assert_eq!(j.verdict.choice, MoodState::Agitated);
            assert_eq!(j.verdict.confidence, 0.95);
            assert_eq!((j.valence, j.need_comfort), (Some(1.0), Some(0.8)));

            let mut odd = response("agitated", 0.95);
            odd.answers.remove(&Question::Comfort);
            odd.answers
                .insert(Question::Valence, Answer::Score { score: 9.0 });
            let j = read_judged(&odd).unwrap();
            assert_eq!((j.valence, j.need_comfort), (Some(4.0), None));

            // 不认识的选项、没有 Q-STATE：整个结果作废
            assert_eq!(read_judged(&response("sleepy", 0.9)), None);
            let mut no_state = response("fluent", 0.9);
            no_state.answers.remove(&Question::State);
            assert_eq!(read_judged(&no_state), None);
        }

        #[tokio::test]
        async fn jev_verdict_drives_fusion_explanation_and_comfort_inputs() {
            let (mut r, gw) = jev_rig(Ok(response("agitated", 0.95)), true);
            r.type_window(1_000);
            // 判断回来之前不融合
            assert!(r.port.saved.lock().unwrap().is_empty());
            r.jev_done().await;

            assert_eq!(gw.reqs.lock().unwrap().len(), 1);
            assert_eq!(r.port.saved.lock().unwrap().len(), 1);
            assert_eq!(r.samples(), [(Some(0.8), Some(1.0), Source::Jev)]);
            // p ≥ 0.9 立即切换，解释带可能性（FR-STA-06 第 4 条、FR-STA-09）
            assert_eq!(r.status().state, MoodState::Agitated);
            let e = r.sense.explanation().unwrap();
            assert_eq!(e.source, ExplainSource::Jev);
            assert_eq!(e.prob_pct, Some(95));
        }

        #[tokio::test]
        async fn without_consent_jev_is_not_asked() {
            let (mut r, gw) = jev_rig(Ok(response("agitated", 0.95)), false);
            r.type_window(1_000);
            assert!(gw.reqs.lock().unwrap().is_empty());
            assert_eq!(r.port.saved.lock().unwrap().len(), 1);
            assert_eq!(r.samples(), [(None, None, Source::Rule)]);
        }

        #[tokio::test]
        async fn failed_judgment_falls_back_to_rules() {
            let (mut r, _) = jev_rig(Err(AiError::Timeout), true);
            r.type_window(1_000);
            r.jev_done().await;
            assert_eq!(r.port.saved.lock().unwrap().len(), 1);
            assert_eq!(r.samples(), [(None, None, Source::Rule)]);
        }

        #[tokio::test]
        async fn windows_wait_in_order_for_the_one_in_flight() {
            let (mut r, gw) = jev_rig(Ok(response("fluent", 0.9)), true);
            r.type_window(1_000);
            r.type_window(10_000);
            // 第二个窗口排队，不另发请求
            assert_eq!((r.sense.pending.len(), r.sense.in_flight), (2, Some(0)));
            assert!(r.port.saved.lock().unwrap().is_empty());
            r.jev_done().await;
            assert_eq!(r.port.saved.lock().unwrap().len(), 1);
            assert_eq!((r.sense.pending.len(), r.sense.in_flight), (1, Some(1)));
            // 撤回 ②：之后的窗口不再问，在途的照常回来
            r.port.jev.store(false, Ordering::SeqCst);
            r.jev_done().await;
            r.type_window(20_000);
            let saved = r.port.saved.lock().unwrap().clone();
            assert_eq!(saved.len(), 3);
            assert!(saved.windows(2).all(|p| p[0].1 <= p[1].0), "{saved:?}");
            assert_eq!(gw.reqs.lock().unwrap().len(), 2);
        }
    }
}
