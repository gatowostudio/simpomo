<script lang="ts">
  import { onMount } from "svelte";
  import * as timer from "./lib/timer";
  import {
    openSettings,
    getSettings,
    onSettingsChanged,
    SECS_PER_MINUTE,
    DEFAULT_FOCUS_BG,
    DEFAULT_BREAK_BG,
    type AppSettings,
  } from "./lib/settings";
  import { setAlwaysOnTop, hideWindow, startResize } from "./lib/window";
  import { playSound, soundsForEvents, type SoundId } from "./lib/sounds";
  import { setBgm, stopBgm, type BgmId } from "./lib/bgm";
  import { textColorFor } from "./lib/color";

  let snap = $state<timer.TimerSnapshot | null>(null);
  let workEndSound = $state<SoundId>("chime");
  let breakEndSound = $state<SoundId>("ding");
  let sessionEndSound = $state<SoundId>("fanfare");
  let volume = $state(70);
  let focusBgm = $state<BgmId>("none");
  let bgmVolume = $state(25);
  let focusBgColor = $state(DEFAULT_FOCUS_BG);
  let breakBgColor = $state(DEFAULT_BREAK_BG);
  // 既定 ON は tauri.conf.json の alwaysOnTop: true と一致させている（spec の中核体験）。
  let alwaysOnTop = $state(true);

  const isRunning = $derived(snap?.status === "running");
  const isBreak = $derived(snap?.phase === "break");
  const phaseLabel = $derived(isBreak ? "BREAK" : "FOCUS");

  // 背景色をフェーズで切替（音が無くても色で分かる）。文字色は背景の明るさから自動でコントラスト。
  const bgColor = $derived(isBreak ? breakBgColor : focusBgColor);
  const fgColor = $derived(textColorFor(bgColor));

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

    // 通知音/BGM の選択・音量を読み込み、設定変更（settings-changed）に追従する。
    const applySoundSettings = (s: AppSettings) => {
      workEndSound = s.workEndSound;
      breakEndSound = s.breakEndSound;
      sessionEndSound = s.sessionEndSound;
      volume = s.volume;
      focusBgm = s.focusBgm;
      bgmVolume = s.bgmVolume;
      focusBgColor = s.focusBgColor;
      breakBgColor = s.breakBgColor;
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

  // フォーカス（作業フェーズ・稼働中）のみ BGM を流す。休憩/一時停止/停止では止める。
  // focusActive は $derived の真偽値なので、毎秒の snapshot 更新では値が変わらず effect は
  // 再実行されない（フェーズや BGM 設定が変わった縁でのみ作用する＝edge 駆動）。
  const focusActive = $derived(snap?.status === "running" && snap?.phase === "work");
  $effect(() => {
    if (focusActive && focusBgm !== "none") setBgm(focusBgm, bgmVolume / 100);
    else stopBgm();
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

<!--
  装飾なし(decorations:false)のため、操作ボタン以外のほぼ全域をドラッグでウィンドウ移動できるようにする。
  Tauri はクリックした要素自体に data-tauri-drag-region が無いとドラッグしないので、背景・表示テキストの
  各要素に付ける（ボタンには付けない＝クリックとして動く）。
-->
<main
  style="background-color: {bgColor}; color: {fgColor};"
  data-tauri-drag-region
>
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

  <div class="display" data-tauri-drag-region>
    <div class="phase" data-tauri-drag-region>{phaseLabel}</div>
    <div class="clock" data-tauri-drag-region>{clock}</div>
    <div class="sets" data-tauri-drag-region>{setLabel}</div>
  </div>

  <div class="controls">
    <button class="primary" onclick={toggleStartPause}>
      {isRunning ? "Pause" : "Start"}
    </button>
    <button onclick={onReset} title="Reset" aria-label="Reset">⟲</button>
    <button onclick={onSkip} title="Skip" aria-label="Skip">⏭</button>
  </div>

  <!-- 端をつまんでリサイズ（フレームレスなので自前ハンドル）。上辺は中央のみ（左右はボタン帯を避ける）。 -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="rz rz-t" onmousedown={() => startResize("North")}></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="rz rz-l" onmousedown={() => startResize("West")}></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="rz rz-r" onmousedown={() => startResize("East")}></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="rz rz-b" onmousedown={() => startResize("South")}></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="rz rz-bl" onmousedown={() => startResize("SouthWest")}></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="rz rz-br" onmousedown={() => startResize("SouthEast")}></div>
</main>

<style>
  /*
    すべてのサイズをウィンドウに比例させる: main の font-size を vmin ベースにし、子は em で組む。
    こうすると端でリサイズしたウィンドウ寸法に応じて文字も一緒に拡縮する。
    上部の操作ボタンは padding-top（= --band）で確保した帯に置き、中央コンテンツと重ならないようにする。
  */
  main {
    --band: 1.7em; /* 上部ボタン帯の高さ。リサイズハンドルの top にも使う。 */
    position: relative;
    height: 100%;
    box-sizing: border-box;
    overflow: hidden; /* 最小付近でも中身がウィンドウ外へはみ出さない安全策。 */
    /* ウィンドウに比例。ただし下限を高めにして小さくても読めるように、上限は大窓向けに大きく。 */
    font-size: clamp(11px, 8vmin, 48px);
    /* 上に操作ボタンの帯ぶんの余白を取り、被りを防ぐ */
    padding: var(--band) 0.5em 0.6em;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.2em;
    user-select: none;
    /* 背景色・文字色はフェーズで切り替わるので滑らかに遷移させる。 */
    transition:
      background-color 0.3s,
      color 0.3s;
  }

  .display {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.1em;
    cursor: default;
  }

  .phase {
    font-size: 0.8em;
    letter-spacing: 0.15em;
    opacity: 0.75;
  }
  .clock {
    font-size: 2.6em;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    line-height: 1.05;
  }
  .sets {
    font-size: 0.72em;
    opacity: 0.6;
  }

  .controls {
    display: flex;
    gap: 0.35em;
    margin-top: 0.35em;
  }
  .controls button {
    font: inherit;
    font-size: 0.78em;
    color: inherit;
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.15);
    border-radius: 0.4em;
    padding: 0.3em 0.6em;
    cursor: pointer;
  }
  .controls button:hover {
    background: rgba(255, 255, 255, 0.16);
  }
  .controls .primary {
    min-width: 5em;
    font-weight: 600;
  }

  .pin,
  .close,
  .gear {
    position: absolute;
    top: 0.35em;
    background: none;
    border: none;
    cursor: pointer;
    color: inherit;
    opacity: 0.35;
    line-height: 1;
    padding: 0.1em;
  }
  .pin {
    right: 0.4em;
    font-size: 0.85em;
    filter: grayscale(1);
  }
  .pin.active {
    opacity: 0.9;
    filter: none;
  }
  .gear {
    right: 1.9em;
    font-size: 0.82em;
  }
  .close {
    left: 0.4em;
    font-size: 0.78em;
  }
  .pin:hover,
  .close:hover,
  .gear:hover {
    opacity: 0.85;
  }

  /* リサイズ用ハンドル（透明・最前面）。左右はボタン帯(--band)を避けて下げる。上辺は中央のみ。 */
  .rz {
    position: absolute;
    z-index: 10;
  }
  .rz-t {
    /* ボタン（左右の隅）を避けた上辺中央だけで上方向リサイズ。 */
    top: 0;
    left: 3em;
    right: 3.4em;
    height: 8px;
    cursor: ns-resize;
  }
  .rz-l,
  .rz-r {
    top: var(--band);
    bottom: 0;
    width: 8px;
    cursor: ew-resize;
  }
  .rz-l {
    left: 0;
  }
  .rz-r {
    right: 0;
  }
  .rz-b {
    left: 0;
    right: 0;
    bottom: 0;
    height: 8px;
    cursor: ns-resize;
  }
  .rz-bl,
  .rz-br {
    bottom: 0;
    width: 16px;
    height: 16px;
    z-index: 11;
  }
  .rz-bl {
    left: 0;
    cursor: nesw-resize;
  }
  .rz-br {
    right: 0;
    cursor: nwse-resize;
  }
</style>
