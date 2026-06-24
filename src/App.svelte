<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import * as timer from "./lib/timer";

  let snap = $state<timer.TimerSnapshot | null>(null);
  // 既定 ON は tauri.conf.json の alwaysOnTop: true と一致させている（spec の中核体験）。
  // 位置/サイズ等の本格的なウィンドウ挙動は #4 で扱う。
  let alwaysOnTop = $state(true);

  const isRunning = $derived(snap?.status === "running");
  const isBreak = $derived(snap?.phase === "break");
  const phaseLabel = $derived(isBreak ? "休憩" : "作業");

  // セット表示: 「現在/総数」。無限は ∞。
  const setLabel = $derived.by(() => {
    if (!snap) return "";
    const current = snap.setIndex + 1;
    return snap.totalSets === null ? `${current} / ∞` : `${current} / ${snap.totalSets}`;
  });

  const clock = $derived(snap ? formatClock(snap.remainingSecs) : "--:--");

  function formatClock(secs: number): string {
    const m = Math.floor(secs / 60)
      .toString()
      .padStart(2, "0");
    const s = (secs % 60).toString().padStart(2, "0");
    return `${m}:${s}`;
  }

  onMount(() => {
    let unlisten: (() => void) | undefined;
    // disposed: onSnapshot の Promise が解決する前に unmount された場合に listener を取りこぼさない。
    let disposed = false;
    // 先に listener を張ってから初期 snapshot を取得し、初期化中の更新を取りこぼさない。
    timer.onSnapshot((s) => (snap = s)).then((u) => {
      if (disposed) u();
      else unlisten = u;
    });
    // 初期値。すでに emit で届いていれば上書きしない。
    timer.getSnapshot().then((s) => {
      if (snap === null) snap = s;
    });
    return () => {
      disposed = true;
      unlisten?.();
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
    alwaysOnTop = !alwaysOnTop;
    await getCurrentWindow().setAlwaysOnTop(alwaysOnTop);
  }
  // トレイ常駐のため「閉じる」はウィンドウを隠すだけ（終了はトレイメニューから）。
  async function onHide() {
    await getCurrentWindow().hide();
  }
</script>

<main class:break={isBreak}>
  <button
    class="pin"
    class:active={alwaysOnTop}
    title={alwaysOnTop ? "最前面: ON" : "最前面: OFF"}
    aria-label="最前面の切り替え"
    onclick={toggleAlwaysOnTop}>📌</button
  >
  <button class="close" title="隠す（トレイに常駐）" aria-label="隠す" onclick={onHide}
    >✕</button
  >

  <!-- 装飾なし(decorations:false)のため、この領域をドラッグでウィンドウ移動できるようにする。 -->
  <div class="display" data-tauri-drag-region>
    <div class="phase">{phaseLabel}</div>
    <div class="clock">{clock}</div>
    <div class="sets">{setLabel}</div>
  </div>

  <div class="controls">
    <button class="primary" onclick={toggleStartPause}>
      {isRunning ? "一時停止" : "開始"}
    </button>
    <button onclick={onReset} title="リセット" aria-label="リセット">⟲</button>
    <button onclick={onSkip} title="スキップ" aria-label="スキップ">⏭</button>
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
  .close {
    position: absolute;
    top: 0.3rem;
    background: none;
    border: none;
    cursor: pointer;
    color: inherit;
    opacity: 0.35;
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
  .close {
    left: 0.3rem;
    font-size: 0.8rem;
    line-height: 1;
  }
  .pin:hover,
  .close:hover {
    opacity: 0.85;
  }
</style>
