//! One bounded worker per physical send-key press. Receipts advance the frozen prefix.
use super::*;
use moodflow_bridge::handler::KeyEventData;

pub(super) struct Sequence {
    generation: u64,
    target: Target,
    started: Instant,
    hold: Duration,
    pace: Duration,
    close_after: bool,
    remaining: usize,
    released: bool,
    long: bool,
    next_at: Instant,
}

impl Inner {
    pub(super) fn sequence_status(&self) -> Option<String> {
        self.sequence.as_ref().map(|sequence| {
            if sequence.long || sequence.released {
                format!("逐块发送 · 剩余 {} 块 · 右键可停止", sequence.remaining)
            } else {
                "松开发送一块；长按逐块发送当前冻结队列".to_owned()
            }
        })
    }
    pub(super) fn stop_sequence(&mut self) {
        self.sequence = None;
        self.buf.release_queue();
    }

    pub(super) fn sequence_receipt(&mut self, outcome: ReceiptOutcome, now: Instant) {
        let Some(sequence) = self.sequence.as_mut() else {
            return;
        };
        if outcome != ReceiptOutcome::Accepted {
            self.stop_sequence();
            return;
        }
        sequence.remaining = sequence.remaining.saturating_sub(1);
        if sequence.remaining == 0 {
            let close = sequence.close_after;
            self.stop_sequence();
            if close {
                self.open = false;
                self.capture_target = None;
                self.buf.suspend();
            }
        } else {
            sequence.next_at = now + sequence.pace;
        }
    }
}

enum Tick {
    Alive,
    Stop,
    Deliver(Delivery),
}

impl BufferState {
    fn release_send_key(&self, key: u32, now: Instant) -> Option<Option<u64>> {
        let mut g = self.lock();
        if g.send_key_down != Some(key) {
            return None;
        }
        g.send_key_down = None;
        Some(g.sequence.as_mut().map(|sequence| {
            // The worker may not have run at the threshold; elapsed hold time is authoritative.
            sequence.long |= now.saturating_duration_since(sequence.started) >= sequence.hold;
            sequence.released = true;
            if !sequence.long {
                sequence.remaining = 1;
            }
            sequence.generation
        }))
    }

    fn sequence_tick(&self, generation: u64, now: Instant, composing: bool) -> Tick {
        let mut g = self.lock();
        let Some(sequence) = g.sequence.as_ref() else {
            return Tick::Stop;
        };
        if sequence.generation != generation {
            return Tick::Stop;
        }
        if !Self::captures(&g) || g.current != Some(sequence.target) {
            g.stop_sequence();
            return Tick::Stop;
        }
        if let Some(pending) = &g.pending {
            if now.saturating_duration_since(pending.started)
                >= Duration::from_millis(DELIVERY_TIMEOUT_MS)
            {
                let pending = g.pending.take().unwrap();
                let _ = g.buf.finish(pending.token, Outcome::Unknown);
                g.stop_sequence();
                g.notice = Some("发送结果不明，已停止逐块投递；请确认后再继续");
                return Tick::Stop;
            }
            return Tick::Alive;
        }
        let sequence = g.sequence.as_mut().unwrap();
        if !sequence.released && !sequence.long {
            if now.saturating_duration_since(sequence.started) < sequence.hold {
                return Tick::Alive;
            }
            sequence.long = true;
        }
        if composing || now < sequence.next_at {
            return Tick::Alive;
        }
        match self.begin_locked(&mut g, false, now) {
            Ok(delivery) => Tick::Deliver(delivery),
            Err(error) => {
                g.stop_sequence();
                g.notice = Some(deliver_error_tip(error));
                Tick::Stop
            }
        }
    }
}

impl Coordinator {
    pub(crate) fn cancel_buffer_sequence(&self) {
        {
            let mut g = self.buffer.lock();
            if let Some(pending) = g.pending.take() {
                let _ = g.buf.finish(pending.token, Outcome::Unknown);
                g.notice = Some("在途发送结果不明，已停止；请确认后再继续");
            } else {
                g.notice = Some("已停止逐块发送，文字已保留");
            }
            g.stop_sequence();
        }
        self.push_buffer_panel();
    }
    pub(crate) fn buffer_send_pressed(&self, data: &KeyEventData) -> Option<KeyAction> {
        if !self.buffer.is_open() {
            return None;
        }
        {
            let mut g = self.buffer.lock();
            // Auto-repeat is still the original physical press, including after queue completion.
            if g.send_key_down == Some(data.key_code) {
                return Some(KeyAction::Consumed);
            }
            g.send_key_down = Some(data.key_code);
        }
        if self.has_pending_composition() {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            return Some(
                self.commit_highlighted(&mut state)
                    .unwrap_or(KeyAction::Consumed),
            );
        }
        if !crate::moodflow::input_privacy().input_allowed() {
            self.buffer_focus_lost();
            return Some(KeyAction::Consumed);
        }
        let (hold, pace, close_after) = {
            let runtime = self.rt();
            let config = &runtime.config.input.buffer;
            (
                Duration::from_millis(config.hold_ms()),
                Duration::from_millis(config.pace_ms()),
                config.close_after_send,
            )
        };
        let generation = {
            let mut g = self.buffer.lock();
            if g.sequence.is_some() || g.pending.is_some() {
                g.notice = Some("正在发送");
                return Some(KeyAction::Consumed);
            }
            if !BufferState::captures(&g) || g.buf.state() != State::Ready {
                g.notice = Some("请先确认上次结果并重新绑定输入框");
                return Some(KeyAction::Consumed);
            }
            let remaining = g.buf.freeze_queue();
            if remaining == 0 {
                return Some(KeyAction::Consumed);
            }
            let generation = self.buffer.next_delivery.fetch_add(1, Ordering::Relaxed);
            let now = Instant::now();
            g.sequence = Some(Sequence {
                generation,
                target: g.current.unwrap(),
                started: now,
                hold,
                pace,
                close_after,
                remaining,
                released: false,
                long: false,
                next_at: now,
            });
            g.notice = Some("松开发送一块；长按逐块发送当前冻结队列");
            generation
        };
        if let Some(weak) = self.self_weak.get().cloned() {
            std::thread::spawn(move || {
                loop {
                    let keep_running = weak
                        .upgrade()
                        .is_some_and(|c| c.buffer_sequence_tick(generation, Instant::now()));
                    if !keep_running {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            });
        }
        self.push_buffer_panel();
        Some(KeyAction::Consumed)
    }

    pub(crate) fn buffer_send_released(&self, data: &KeyEventData) -> Option<KeyAction> {
        let generation = self
            .buffer
            .release_send_key(data.key_code, Instant::now())?;
        if let Some(generation) = generation {
            self.buffer_sequence_tick(generation, Instant::now());
        }
        Some(KeyAction::Consumed)
    }

    fn buffer_sequence_tick(&self, generation: u64, now: Instant) -> bool {
        if !crate::moodflow::input_privacy().input_allowed() {
            self.buffer_focus_lost();
            return false;
        }
        let composing = self.has_pending_composition();
        match self.buffer.sequence_tick(generation, now, composing) {
            Tick::Deliver(delivery) => {
                self.dispatch_buffer_delivery(delivery, false);
                self.push_buffer_panel();
                true
            }
            Tick::Alive => true,
            Tick::Stop => {
                self.push_buffer_panel();
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moodflow_keys::keymap::VK_RETURN;

    const TARGET: Target = Target::new(73, 9);
    const GENERATION: u64 = 11;

    fn setup(pace_ms: u64, close: bool) -> (BufferState, Instant) {
        let state = BufferState::default();
        state.focus_gained(TARGET.id, TARGET.revision);
        let now = Instant::now();
        {
            let mut g = state.lock();
            g.open = true;
            g.capture_target = Some(TARGET);
            g.buf.bind(TARGET, FieldKind::Normal).unwrap();
            g.buf.push_phrase("甲").unwrap();
            g.buf.push_latin('a').unwrap();
            let remaining = g.buf.freeze_queue();
            g.send_key_down = Some(VK_RETURN);
            g.sequence = Some(Sequence {
                generation: GENERATION,
                target: TARGET,
                started: now,
                hold: Duration::from_millis(1200),
                pace: Duration::from_millis(pace_ms),
                close_after: close,
                remaining,
                released: false,
                long: false,
                next_at: now,
            });
        }
        (state, now)
    }

    fn deliver(state: &BufferState, now: Instant) -> Delivery {
        match state.sequence_tick(GENERATION, now, false) {
            Tick::Deliver(delivery) => delivery,
            _ => panic!("expected a delivery"),
        }
    }

    fn finish(state: &BufferState, delivery: &Delivery, outcome: ReceiptOutcome, now: Instant) {
        if outcome == ReceiptOutcome::Accepted {
            assert!(state.authorize(delivery.id, delivery.client, delivery.revision));
        }
        assert!(state.finish_at(
            Receipt {
                id: delivery.id,
                client: delivery.client,
                revision: delivery.revision,
                outcome
            },
            now
        ));
    }

    #[test]
    fn short_press_sends_one_block_after_release() {
        let (state, now) = setup(0, false);
        assert!(matches!(
            state.sequence_tick(GENERATION, now + Duration::from_millis(299), false),
            Tick::Alive
        ));
        assert!(state.lock().pending.is_none());
        assert_eq!(
            state.release_send_key(VK_RETURN, now + Duration::from_millis(300)),
            Some(Some(GENERATION))
        );
        assert!(
            state
                .release_send_key(VK_RETURN, now + Duration::from_millis(300))
                .is_none(),
            "double TSF key-up must be inert"
        );
        let d = deliver(&state, now + Duration::from_millis(300));
        assert_eq!(d.text, "甲");
        finish(
            &state,
            &d,
            ReceiptOutcome::Accepted,
            now + Duration::from_millis(310),
        );
        let g = state.lock();
        assert!(g.sequence.is_none());
        assert_eq!(g.buf.text(), "a");
        assert_eq!(g.buf.frozen_queue_len(), 0);
    }

    #[test]
    fn release_after_threshold_is_long_even_if_worker_was_delayed() {
        let (state, now) = setup(0, false);
        state.release_send_key(VK_RETURN, now + Duration::from_millis(1200));
        let first = deliver(&state, now + Duration::from_millis(1200));
        finish(
            &state,
            &first,
            ReceiptOutcome::Accepted,
            now + Duration::from_millis(1201),
        );
        assert_eq!(deliver(&state, now + Duration::from_millis(1201)).text, "a");
    }

    #[test]
    fn long_press_waits_for_receipts_and_paces_from_acceptance() {
        let (state, now) = setup(2000, false);
        state.lock().buf.push_latin('b').unwrap();
        let first = deliver(&state, now + Duration::from_millis(1200));
        assert_eq!(first.text, "甲");
        assert!(matches!(
            state.sequence_tick(GENERATION, now + Duration::from_millis(1800), false),
            Tick::Alive
        ));
        finish(
            &state,
            &first,
            ReceiptOutcome::Accepted,
            now + Duration::from_millis(2000),
        );
        assert!(matches!(
            state.sequence_tick(GENERATION, now + Duration::from_millis(3999), false),
            Tick::Alive
        ));
        let second = deliver(&state, now + Duration::from_millis(4000));
        assert_eq!(second.text, "a");
        finish(
            &state,
            &second,
            ReceiptOutcome::Accepted,
            now + Duration::from_millis(4010),
        );
        let g = state.lock();
        assert!(g.sequence.is_none());
        assert_eq!(
            g.buf.text(),
            "b",
            "later text must remain outside the frozen queue"
        );
        assert_eq!(
            g.send_key_down,
            Some(VK_RETURN),
            "autorepeat stays latched until physical release"
        );
    }

    #[test]
    fn reject_and_unknown_stop_the_sequence_without_removing_text() {
        for outcome in [ReceiptOutcome::Rejected, ReceiptOutcome::Unknown] {
            let (state, now) = setup(0, false);
            let d = deliver(&state, now + Duration::from_millis(1200));
            finish(&state, &d, outcome, now + Duration::from_millis(1300));
            let g = state.lock();
            assert!(g.sequence.is_none());
            assert_eq!(g.buf.text(), "甲a");
            assert_eq!(g.buf.frozen_queue_len(), 0);
            assert_eq!(
                g.buf.state(),
                if outcome == ReceiptOutcome::Unknown {
                    State::Held
                } else {
                    State::Ready
                }
            );
        }
    }

    #[test]
    fn timeout_and_focus_change_revoke_the_remaining_queue() {
        let (state, now) = setup(0, false);
        let _ = deliver(&state, now + Duration::from_millis(1200));
        assert!(matches!(
            state.sequence_tick(GENERATION, now + Duration::from_millis(6200), false),
            Tick::Stop
        ));
        assert_eq!(state.lock().buf.state(), State::Held);
        assert_eq!(state.lock().buf.text(), "甲a");
        let (state, now) = setup(0, false);
        state.focus_gained(TARGET.id, TARGET.revision + 1);
        assert!(matches!(
            state.sequence_tick(GENERATION, now + Duration::from_secs(2), false),
            Tick::Stop
        ));
        assert_eq!(state.lock().buf.text(), "甲a");
    }

    #[test]
    fn composing_pauses_and_completion_can_close_while_preserving_new_text() {
        let (state, now) = setup(0, true);
        assert!(matches!(
            state.sequence_tick(GENERATION, now + Duration::from_secs(2), true),
            Tick::Alive
        ));
        assert!(state.lock().pending.is_none());
        let first = deliver(&state, now + Duration::from_secs(2));
        finish(
            &state,
            &first,
            ReceiptOutcome::Accepted,
            now + Duration::from_secs(2),
        );
        state.lock().buf.push_phrase("新增").unwrap();
        let second = deliver(&state, now + Duration::from_secs(2));
        finish(
            &state,
            &second,
            ReceiptOutcome::Accepted,
            now + Duration::from_secs(2),
        );
        let g = state.lock();
        assert!(!g.open);
        assert!(g.capture_target.is_none());
        assert_eq!(g.buf.text(), "新增");
    }
}
