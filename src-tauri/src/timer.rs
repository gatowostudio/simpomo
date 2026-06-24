//! ポモドーロタイマーのコアロジック（状態機械）。
//!
//! 設計方針（ADR-0002）: タイマーの「真実」は Rust が持つ。ここは時計に一切依存しない
//! 純粋な状態機械として実装し、`tick(elapsed)` で時間を進める。本モジュールは単体テストできる。
//!
//! 駆動の契約（重要）: `tick` を呼ぶのは **Rust 側のランタイムタスク**（#3）であって、
//! フロント（WebView）ではない。フロントはイベント受信と `invoke` のみを行う薄い表示層。
//! ランタイムは「前回 tick からの実経過秒」を**単調時計（`std::time::Instant` の差分）**で
//! 算出して渡すこと。固定の `tick(1)` をインターバルで呼ぶ素朴実装はドリフトし、スリープ
//! 復帰時に実時間とずれる。`tick` が複数フェーズ境界を一度に処理できる（スリープ復帰で
//! まとめて経過秒を渡せる）のはこのため。
//! ※ tick(秒)モデルを採用し deadline モデルは採らない。ポモドーロは精度非クリティカルで、
//!   単調時計駆動ならドリフト/スリープ問題は実用上回避できるため（判断の詳細は ADR-0002）。
//!
//! サイクル数（spec）: `0`=1 セット（作業→休憩）で停止し手動 start 待ち / 有限 `N`=N セット
//! 自動連続で停止 / 無限=停止せず継続。

use serde::Serialize;

/// 既定の作業時間（25 分）。
pub const DEFAULT_WORK_SECS: u32 = 25 * 60;
/// 既定の休憩時間（5 分）。
pub const DEFAULT_BREAK_SECS: u32 = 5 * 60;
/// 1 フェーズの最小秒数。0 秒フェーズは tick の無限ループを招くため下限を設ける。
pub const MIN_PHASE_SECS: u32 = 1;

/// 現在のフェーズ（作業 / 休憩）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Work,
    Break,
}

/// タイマーの稼働状態。
///
/// `Idle` は「未開始 / 停止中（start 待ち）」を表す。セッション完了後もここへ戻り、
/// 次の start で新しいセッションが始まる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Idle,
    Running,
    Paused,
}

/// 自動継続するサイクル（セット）数の設定。
///
/// - `Finite(0)`: 既定。1 セットだけ実行して停止（手動 start 待ち）。
/// - `Finite(n)` (n>=1): n セットを自動連続実行して停止。
/// - `Infinite`: 停止せず自動継続。
///
/// 注: 仕様上 `Finite(0)` と `Finite(1)` は「1 セットで停止」という同一挙動になる。
/// `0` は「自動継続なし」を表す既定値としての綴りであり、意図的な等価である。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleSetting {
    Finite(u32),
    Infinite,
}

impl CycleSetting {
    /// 実際に走らせるセット数。`Infinite` は上限なしを表す `None`。
    ///
    /// 注: ここで `Finite(0)` を `Some(1)` に畳むのは「走らせる回数」を求めるときだけ。
    /// 設定値そのもの（既定の `0` を含む）は `Config` に保持され、#5 の永続化/設定 UI とは
    /// その生値で往復する（`0` が `1` に化けない）。
    fn target_sets(self) -> Option<u32> {
        match self {
            CycleSetting::Finite(0) => Some(1),
            CycleSetting::Finite(n) => Some(n),
            CycleSetting::Infinite => None,
        }
    }

    /// `completed` セットを完了した時点でセッションを終えるべきか。無限設定では終わらない。
    fn is_last_set(self, completed: u32) -> bool {
        match self.target_sets() {
            Some(target) => completed >= target,
            None => false,
        }
    }
}

/// タイマーの設定値。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    pub work_secs: u32,
    pub break_secs: u32,
    pub cycles: CycleSetting,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            work_secs: DEFAULT_WORK_SECS,
            break_secs: DEFAULT_BREAK_SECS,
            cycles: CycleSetting::Finite(0),
        }
    }
}

impl Config {
    /// 不正値（0 秒フェーズ）を最小値へ丸めた健全な設定を返す。
    fn sanitized(self) -> Self {
        Self {
            work_secs: self.work_secs.max(MIN_PHASE_SECS),
            break_secs: self.break_secs.max(MIN_PHASE_SECS),
            cycles: self.cycles,
        }
    }
}

/// フェーズ境界で発生する出来事。呼び出し側が通知音（#6）などに使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TimerEvent {
    /// 作業フェーズが終了した（→ 休憩へ）。タイマー終了音の契機。
    WorkEnded,
    /// 休憩フェーズが終了した。休憩終了音の契機。
    BreakEnded,
    /// 設定されたセット数を完了し、セッションが停止した（Idle に戻った）。
    SessionFinished,
}

/// フロントへ渡す表示用スナップショット。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerSnapshot {
    pub phase: Phase,
    pub status: Status,
    pub remaining_secs: u32,
    /// 現在のセット番号（0 始まり）。表示時は +1 する想定。
    pub set_index: u32,
    /// 走らせるセット総数。無限のときは `None`。
    pub total_sets: Option<u32>,
    pub work_secs: u32,
    pub break_secs: u32,
}

/// ポモドーロタイマーの状態機械。
#[derive(Debug, Clone, Copy)]
pub struct Timer {
    config: Config,
    phase: Phase,
    status: Status,
    remaining: u32,
    /// 現在のセット番号（0 始まり）。「これまで完了したセット数」は `set_index + 1` で表す。
    set_index: u32,
}

impl Timer {
    /// 設定からタイマーを生成する。初期状態は「作業フェーズの先頭で停止（Idle）」。
    pub fn new(config: Config) -> Self {
        let config = config.sanitized();
        Self {
            config,
            phase: Phase::Work,
            status: Status::Idle,
            remaining: config.work_secs,
            set_index: 0,
        }
    }

    /// 現在状態のスナップショットを返す。
    pub fn snapshot(&self) -> TimerSnapshot {
        TimerSnapshot {
            phase: self.phase,
            status: self.status,
            remaining_secs: self.remaining,
            set_index: self.set_index,
            total_sets: self.config.cycles.target_sets(),
            work_secs: self.config.work_secs,
            break_secs: self.config.break_secs,
        }
    }

    /// 現在の設定を返す。
    pub fn config(&self) -> Config {
        self.config
    }

    /// 設定を差し替える。
    ///
    /// 停止中（Idle）は次セッションの先頭に新しい作業時間を反映するため reset する。
    /// 実行中 / 一時停止中は進行中セッションを壊さない: 新しい各フェーズ時間は次フェーズ以降、
    /// サイクル数は次の終了判定から反映される。これにより別ウィンドウ（ADR-0002）で設定を
    /// 保存しても走行中のタイマーが巻き戻らない。
    pub fn set_config(&mut self, config: Config) {
        self.config = config.sanitized();
        if self.status == Status::Idle {
            self.reset();
        }
    }

    /// 開始 / 再開する。
    /// - Idle からは新しいセッションを最初（作業フェーズ）から始める。
    /// - Paused からは再開する。
    /// - Running のときは何もしない。
    pub fn start(&mut self) {
        match self.status {
            Status::Running => {}
            Status::Paused => self.status = Status::Running,
            Status::Idle => {
                self.phase = Phase::Work;
                self.remaining = self.config.work_secs;
                self.set_index = 0;
                self.status = Status::Running;
            }
        }
    }

    /// 一時停止する（Running のときのみ）。
    pub fn pause(&mut self) {
        if self.status == Status::Running {
            self.status = Status::Paused;
        }
    }

    /// 最初の状態（作業フェーズ先頭・停止）へ戻す。
    pub fn reset(&mut self) {
        self.phase = Phase::Work;
        self.remaining = self.config.work_secs;
        self.set_index = 0;
        self.status = Status::Idle;
    }

    /// 現在フェーズを即座に終了して次へ進める。稼働状態（Running/Paused）は維持する。
    /// Idle のときは何もしない。跨いだフェーズ境界の `TimerEvent` を返す。
    pub fn skip(&mut self) -> Vec<TimerEvent> {
        if self.status == Status::Idle {
            return Vec::new();
        }
        let mut events = Vec::new();
        self.complete_phase(&mut events);
        events
    }

    /// `secs` 秒だけ時間を進める。Running 以外では何も起きない。
    /// 跨いだフェーズ境界の `TimerEvent` を発生順に返す（システムスリープ等で複数境界を
    /// 一度に跨ぐ場合も正しく処理する）。
    pub fn tick(&mut self, secs: u32) -> Vec<TimerEvent> {
        let mut events = Vec::new();
        if self.status != Status::Running {
            return events;
        }
        let mut left = secs;
        // フェーズ秒数は >= MIN_PHASE_SECS のため、各反復で必ず時間を消費し停止する。
        while left > 0 && self.status == Status::Running {
            if left < self.remaining {
                self.remaining -= left;
                left = 0;
            } else {
                left -= self.remaining;
                self.remaining = 0;
                self.complete_phase(&mut events);
            }
        }
        events
    }

    /// 現在フェーズの完了時遷移。作業→休憩、休憩→次セットの作業 or セッション終了。
    fn complete_phase(&mut self, events: &mut Vec<TimerEvent>) {
        match self.phase {
            Phase::Work => {
                events.push(TimerEvent::WorkEnded);
                self.phase = Phase::Break;
                self.remaining = self.config.break_secs;
            }
            Phase::Break => {
                events.push(TimerEvent::BreakEnded);
                let completed = self.set_index.saturating_add(1);
                if self.config.cycles.is_last_set(completed) {
                    events.push(TimerEvent::SessionFinished);
                    self.reset();
                } else {
                    self.set_index = completed;
                    self.phase = Phase::Work;
                    self.remaining = self.config.work_secs;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(work: u32, brk: u32, cycles: CycleSetting) -> Config {
        Config {
            work_secs: work,
            break_secs: brk,
            cycles,
        }
    }

    #[test]
    fn default_config_is_25_5_and_finite_zero() {
        // 既定が「作業25分=1500秒 / 休憩5分=300秒」であることを生リテラルで固定する。
        let c = Config::default();
        assert_eq!(c.work_secs, 1500);
        assert_eq!(c.break_secs, 300);
        assert_eq!(c.cycles, CycleSetting::Finite(0));
    }

    #[test]
    fn new_timer_starts_idle_at_work() {
        let t = Timer::new(Config::default());
        let s = t.snapshot();
        assert_eq!(s.status, Status::Idle);
        assert_eq!(s.phase, Phase::Work);
        assert_eq!(s.remaining_secs, 1500);
        assert_eq!(s.set_index, 0);
        assert_eq!(s.total_sets, Some(1));
    }

    #[test]
    fn tick_while_idle_does_nothing() {
        let mut t = Timer::new(Config::default());
        let events = t.tick(60);
        assert!(events.is_empty());
        assert_eq!(t.snapshot().remaining_secs, 1500);
        assert_eq!(t.snapshot().status, Status::Idle);
    }

    #[test]
    fn start_then_tick_counts_down() {
        let mut t = Timer::new(config(10, 5, CycleSetting::Infinite));
        t.start();
        let events = t.tick(3);
        assert!(events.is_empty());
        assert_eq!(t.snapshot().remaining_secs, 7);
        assert_eq!(t.snapshot().status, Status::Running);
    }

    #[test]
    fn work_completes_to_break() {
        let mut t = Timer::new(config(10, 5, CycleSetting::Infinite));
        t.start();
        let events = t.tick(10);
        assert_eq!(events, vec![TimerEvent::WorkEnded]);
        let s = t.snapshot();
        assert_eq!(s.phase, Phase::Break);
        assert_eq!(s.remaining_secs, 5);
        assert_eq!(s.set_index, 0);
    }

    #[test]
    fn cycles_zero_runs_exactly_one_set_then_idle() {
        let mut t = Timer::new(config(10, 5, CycleSetting::Finite(0)));
        t.start();
        assert_eq!(t.tick(10), vec![TimerEvent::WorkEnded]);
        let events = t.tick(5);
        assert_eq!(
            events,
            vec![TimerEvent::BreakEnded, TimerEvent::SessionFinished]
        );
        let s = t.snapshot();
        assert_eq!(s.status, Status::Idle);
        assert_eq!(s.phase, Phase::Work);
        assert_eq!(s.set_index, 0);
        assert_eq!(s.remaining_secs, 10);
    }

    #[test]
    fn cycles_zero_and_one_are_equivalent() {
        // どちらも 1 セットで停止する（仕様上の意図的等価）。
        for cycles in [CycleSetting::Finite(0), CycleSetting::Finite(1)] {
            let mut t = Timer::new(config(2, 2, cycles));
            t.start();
            let mut all = t.tick(2);
            all.extend(t.tick(2));
            assert!(all.contains(&TimerEvent::SessionFinished), "{cycles:?}");
            assert_eq!(t.snapshot().status, Status::Idle, "{cycles:?}");
        }
    }

    #[test]
    fn finite_n_runs_n_sets_then_stops() {
        let mut t = Timer::new(config(2, 2, CycleSetting::Finite(3)));
        t.start();
        // 2 セット分（作業+休憩）×2 を消化しても終わらない。
        for expected_set in 0..2u32 {
            assert_eq!(t.tick(2), vec![TimerEvent::WorkEnded]);
            assert_eq!(t.tick(2), vec![TimerEvent::BreakEnded]);
            assert_eq!(t.snapshot().set_index, expected_set + 1);
            assert_eq!(t.snapshot().status, Status::Running);
        }
        // 3 セット目の休憩終了でセッション完了。
        assert_eq!(t.tick(2), vec![TimerEvent::WorkEnded]);
        assert_eq!(
            t.tick(2),
            vec![TimerEvent::BreakEnded, TimerEvent::SessionFinished]
        );
        assert_eq!(t.snapshot().status, Status::Idle);
        assert_eq!(t.snapshot().set_index, 0);
    }

    #[test]
    fn infinite_never_finishes() {
        let mut t = Timer::new(config(1, 1, CycleSetting::Infinite));
        t.start();
        for _ in 0..50 {
            let mut events = t.tick(1); // work end
            events.extend(t.tick(1)); // break end
            assert!(!events.contains(&TimerEvent::SessionFinished));
            assert_eq!(t.snapshot().status, Status::Running);
        }
        assert_eq!(t.snapshot().set_index, 50);
        assert_eq!(t.snapshot().total_sets, None);
    }

    #[test]
    fn pause_stops_countdown_and_start_resumes() {
        let mut t = Timer::new(config(10, 5, CycleSetting::Infinite));
        t.start();
        t.tick(3);
        t.pause();
        assert_eq!(t.snapshot().status, Status::Paused);
        assert!(t.tick(100).is_empty());
        assert_eq!(t.snapshot().remaining_secs, 7); // 進まない
        t.start(); // 再開
        assert_eq!(t.snapshot().status, Status::Running);
        t.tick(7);
        assert_eq!(t.snapshot().phase, Phase::Break);
    }

    #[test]
    fn reset_returns_to_idle_work_head() {
        let mut t = Timer::new(config(10, 5, CycleSetting::Finite(3)));
        t.start();
        t.tick(10); // → break
        t.reset();
        let s = t.snapshot();
        assert_eq!(s.status, Status::Idle);
        assert_eq!(s.phase, Phase::Work);
        assert_eq!(s.remaining_secs, 10);
        assert_eq!(s.set_index, 0);
    }

    #[test]
    fn skip_work_jumps_to_break() {
        let mut t = Timer::new(config(10, 5, CycleSetting::Infinite));
        t.start();
        let events = t.skip();
        assert_eq!(events, vec![TimerEvent::WorkEnded]);
        assert_eq!(t.snapshot().phase, Phase::Break);
        assert_eq!(t.snapshot().remaining_secs, 5);
    }

    #[test]
    fn skip_last_break_finishes_session() {
        let mut t = Timer::new(config(10, 5, CycleSetting::Finite(0)));
        t.start();
        assert_eq!(t.skip(), vec![TimerEvent::WorkEnded]); // → break
        let events = t.skip(); // break → finish
        assert_eq!(
            events,
            vec![TimerEvent::BreakEnded, TimerEvent::SessionFinished]
        );
        assert_eq!(t.snapshot().status, Status::Idle);
    }

    #[test]
    fn skip_while_idle_is_noop() {
        let mut t = Timer::new(Config::default());
        assert!(t.skip().is_empty());
        assert_eq!(t.snapshot().status, Status::Idle);
    }

    #[test]
    fn skip_while_paused_advances_phase_but_stays_paused() {
        let mut t = Timer::new(config(10, 5, CycleSetting::Infinite));
        t.start();
        t.tick(3);
        t.pause();
        let events = t.skip();
        assert_eq!(events, vec![TimerEvent::WorkEnded]);
        let s = t.snapshot();
        assert_eq!(s.phase, Phase::Break);
        assert_eq!(s.remaining_secs, 5);
        assert_eq!(s.status, Status::Paused); // skip しても一時停止は維持
    }

    #[test]
    fn skip_final_break_while_paused_finishes_to_idle() {
        // 最終セットの休憩を Paused 中に skip すると、SessionFinished で Idle へ落ちる
        // （Paused という稼働状態が reset により消える非自明な副作用を固定する）。
        let mut t = Timer::new(config(10, 5, CycleSetting::Finite(0)));
        t.start();
        assert_eq!(t.skip(), vec![TimerEvent::WorkEnded]); // work → break (Running)
        t.pause();
        let events = t.skip(); // break(最終) → finish
        assert_eq!(
            events,
            vec![TimerEvent::BreakEnded, TimerEvent::SessionFinished]
        );
        assert_eq!(t.snapshot().status, Status::Idle);
    }

    #[test]
    fn start_while_running_is_noop() {
        let mut t = Timer::new(config(10, 5, CycleSetting::Infinite));
        t.start();
        t.tick(3);
        t.start(); // 進行中の start は無視され、巻き戻らない
        assert_eq!(t.snapshot().remaining_secs, 7);
        assert_eq!(t.snapshot().status, Status::Running);
    }

    #[test]
    fn tick_across_multiple_boundaries_in_one_call() {
        // work=2, break=1, 2 セット。10 秒一気に進めて完了まで到達する。
        let mut t = Timer::new(config(2, 1, CycleSetting::Finite(2)));
        t.start();
        let events = t.tick(10);
        // 1set: WorkEnded,BreakEnded / 2set: WorkEnded,BreakEnded+SessionFinished
        assert_eq!(
            events,
            vec![
                TimerEvent::WorkEnded,
                TimerEvent::BreakEnded,
                TimerEvent::WorkEnded,
                TimerEvent::BreakEnded,
                TimerEvent::SessionFinished,
            ]
        );
        assert_eq!(t.snapshot().status, Status::Idle);
        // 完了後は Idle なので、余った時間は消費されず巻き込まれない。
        assert_eq!(t.snapshot().remaining_secs, 2);
    }

    #[test]
    fn zero_duration_config_is_clamped_and_safe() {
        // 0 秒設定は最小 1 秒に丸められ、tick が無限ループしない。
        let mut t = Timer::new(config(0, 0, CycleSetting::Infinite));
        let s = t.snapshot();
        assert_eq!(s.work_secs, MIN_PHASE_SECS);
        assert_eq!(s.break_secs, MIN_PHASE_SECS);
        t.start();
        let events = t.tick(3); // 3 秒で複数境界を跨ぐが停止する
        assert!(events.contains(&TimerEvent::WorkEnded));
    }

    #[test]
    fn snapshot_total_sets_reflects_setting() {
        assert_eq!(
            Timer::new(config(1, 1, CycleSetting::Finite(0)))
                .snapshot()
                .total_sets,
            Some(1)
        );
        assert_eq!(
            Timer::new(config(1, 1, CycleSetting::Finite(4)))
                .snapshot()
                .total_sets,
            Some(4)
        );
        assert_eq!(
            Timer::new(config(1, 1, CycleSetting::Infinite))
                .snapshot()
                .total_sets,
            None
        );
    }

    #[test]
    fn set_config_while_idle_applies_and_resets() {
        let mut t = Timer::new(config(10, 5, CycleSetting::Infinite));
        t.set_config(config(30, 10, CycleSetting::Finite(2)));
        let s = t.snapshot();
        assert_eq!(s.status, Status::Idle);
        assert_eq!(s.remaining_secs, 30); // 次セッション先頭に新しい作業時間が反映
        assert_eq!(s.work_secs, 30);
        assert_eq!(s.break_secs, 10);
        assert_eq!(s.total_sets, Some(2));
    }

    #[test]
    fn set_config_while_running_keeps_session_and_applies_to_future() {
        let mut t = Timer::new(config(10, 5, CycleSetting::Infinite));
        t.start();
        t.tick(3); // 作業フェーズ、残り 7
        t.set_config(config(30, 8, CycleSetting::Finite(2)));
        let s = t.snapshot();
        // 進行中セッションは壊さない: 現フェーズの残り時間・状態は保持される。
        assert_eq!(s.status, Status::Running);
        assert_eq!(s.phase, Phase::Work);
        assert_eq!(s.remaining_secs, 7);
        // 新しい時間は次フェーズ以降に反映: 現作業を skip すると休憩は新しい 8 秒。
        assert_eq!(t.skip(), vec![TimerEvent::WorkEnded]);
        assert_eq!(t.snapshot().remaining_secs, 8);
        // サイクル数は即時反映される。
        assert_eq!(t.snapshot().total_sets, Some(2));
    }
}
