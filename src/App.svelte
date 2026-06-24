<script lang="ts">
  import { onMount } from "svelte";
  import * as timer from "./lib/timer";
  import {
    openSettings,
    getSettings,
    onSettingsChanged,
    SECS_PER_MINUTE,
  } from "./lib/settings";
  import { setAlwaysOnTop, hideWindow } from "./lib/window";
  import { playSound, soundsForEvents, type SoundId } from "./lib/sounds";

  let snap = $state<timer.TimerSnapshot | null>(null);
  let workEndSound = $state<SoundId>("chime");
  let breakEndSound = $state<SoundId>("ding");
  let sessionEndSound = $state<SoundId>("fanfare");
  let volume = $state(70);
  // 既定 ON は tauri.conf.json の alwaysOnTop: true と一致させている（spec の中核体験）。
  // 位置/サイズ等の本格的なウィンドウ挙動は #4 で扱う。
  let alwaysOnTop = $state(true);

  const isRunning = $derived(snap?.status === "running");
  const isBreak = $derived(snap?.phase === "break");
  const phaseLabel = $derived(isBreak ? "BREAK" : "FOCUS");

  // セット表示: 「現在/総数」。無限は ∞。
  const setLabel = $derived.by(() => {
    if (!snap) return "";
    const current = snap.setIndex + 1;
    return snap.totalSets === null ? `${current} / ∞` : `${current} / ${snap.totalSets}`;
  });

  const clock = $derived(snap ? formatClock(snap.remainingSecs) : "--:--");

  function formatClock(secs: number): string {
    const m = Math.floor(secs / SECS_PER_MINUTE)
      .toString()
      .padStart(2, "0");
    const s = (secs % SECS_PER_MINUTE).toString().padStart(2, "0");
    return `${m}:${s}`;
  }

  onMount(() => {
    const unlisteners: Array<() => void> = [];
    // disposed: listen の Promise が解決する前に unmount された場合に listener を取りこぼさない。
    let disposed = false;
    const track = (p: Promise<() => void>) => {
      p.then((u) => (disposed ? u() : unlisteners.push(u)));
    };

    // 先に listener を張ってから初期 snapshot を取得し、初期化中の更新を取りこぼさない。
    track(timer.onSnapshot((s) => (snap = s)));
    timer.getSnapshot().then((s) => {
      if (snap === null) snap = s;
    });

    // 通知音の選択・音量を読み込み、設定変更（settings-changed）に追従する。
    const applySoundSettings = (s: {
      workEndSound: SoundId;
      breakEndSound: SoundId;
      sessionEndSound: SoundId;
      volume: number;
    }) => {
      workEndSound = s.workEndSound;
      breakEndSound = s.breakEndSound;
      sessionEndSound = s.sessionEndSound;
      volume = s.volume;
    };
    getSettings().then(applySoundSettings).catch(() => {});
    track(onSettingsChanged(applySoundSettings));

    // フェーズ境界で通知音を鳴らす。完了時は完了音、catch-up（複数境界）は種類ごと 1 回に畳む。
    // 手動 skip は Rust 側でイベントを出さないので鳴らない。
    track(
      timer.onTimerEvents((events) => {
        for (const id of soundsForEvents(events, {
          work: workEndSound,
          break: breakEndSound,
          session: sessionEndSound,
        })) {
          playSound(id, volume / 100);
        }
      }),
    );

    return () => {
      disposed = true;
      unlisteners.forEach((u) => u());
    };
  });

  // 状態更新は onSnapshot（emit）の単一経路。コマンドは結果を代入しない。
  async function toggleStartPause() {
    await (isRunning ? timer.pause() : timer.start());
  }
  async function onReset() {
    await timer.reset();
  }
  async function onSkip() {
    await timer.skip();
  }
  async function toggleAlwaysOnTop() {
    const next = !alwaysOnTop;
    try {
      await setAlwaysOnTop(next);
      alwaysOnTop = next; // 実際に適用できたときだけ表示状態を更新する
    } catch (e) {
      console.error("failed to toggle always-on-top", e);
    }
  }
  // トレイ常駐のため「閉じる」はウィンドウを隠すだけ（終了はトレイメニューから）。
  async function onHide() {
    await hideWindow();
  }
  async function onOpenSettings() {
    // 連打などで生成が競合しても UI を壊さないよう失敗は握りつぶす。
    try {
      await openSettings();
    } catch (e) {
      console.error("failed to open settings", e);
    }
  }
</script>

<main class:break={isBreak}>
  <button
    class="pin"
    class:active={alwaysOnTop}
    title={alwaysOnTop ? "Always on top: ON" : "Always on top: OFF"}
    aria-label="Toggle always on top"
    onclick={toggleAlwaysOnTop}>📌</button
  >
  <button class="close" title="Hide to tray" aria-label="Hide" onclick={onHide}
    >✕</button
  >
  <button class="gear" title="Settings" aria-label="Settings" onclick={onOpenSettings}
    >⚙</button
  >

  <!-- 装飾なし(decorations:false)のため、この領域をドラッグでウィンドウ移動できるようにする。 -->
  <div class="display" data-tauri-drag-region>
    <div class="phase">{phaseLabel}</div>
    <div class="clock">{clock}</div>
    <div class="sets">{setLabel}</div>
  </div>

  <div class="controls">
    <button class="primary" onclick={toggleStartPause}>
      {isRunning ? "Pause" : "Start"}
    </button>
    <button onclick={onReset} title="Reset" aria-label="Reset">⟲</button>
    <button onclick={onSkip} title="Skip" aria-label="Skip">⏭</button>
  </div>
</main>

<style>
  main {
    position: relative;
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.25rem;
    user-select: none;
    transition: color 0.3s;
  }
  main.break {
    color: #6cc070; /* 休憩中は緑寄りに */
  }

  .display {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.15rem;
    cursor: default;
  }

  .phase {
    font-size: 0.8rem;
    letter-spacing: 0.15em;
    opacity: 0.75;
  }
  .clock {
    font-size: 2.6rem;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    line-height: 1.1;
  }
  .sets {
    font-size: 0.75rem;
    opacity: 0.6;
  }

  .controls {
    display: flex;
    gap: 0.4rem;
    margin-top: 0.4rem;
  }
  .controls button {
    font: inherit;
    color: inherit;
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.15);
    border-radius: 6px;
    padding: 0.3rem 0.6rem;
    cursor: pointer;
  }
  .controls button:hover {
    background: rgba(255, 255, 255, 0.16);
  }
  .controls .primary {
    min-width: 5.5rem;
    font-weight: 600;
  }

  .pin,
  .close,
  .gear {
    position: absolute;
    top: 0.3rem;
    background: none;
    border: none;
    cursor: pointer;
    color: inherit;
    opacity: 0.35;
    line-height: 1;
  }
  .pin {
    right: 0.3rem;
    font-size: 0.9rem;
    filter: grayscale(1);
  }
  .pin.active {
    opacity: 0.9;
    filter: none;
  }
  .gear {
    right: 1.7rem;
    font-size: 0.85rem;
  }
  .close {
    left: 0.3rem;
    font-size: 0.8rem;
  }
  .pin:hover,
  .close:hover,
  .gear:hover {
    opacity: 0.85;
  }
</style>
