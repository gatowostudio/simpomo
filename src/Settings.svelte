<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import * as settings from "./lib/settings";
  import { hideWindow } from "./lib/window";
  import { playSound, type SoundId } from "./lib/sounds";
  import { setBgm, stopBgm, type BgmId } from "./lib/bgm";

  // 編集中のフォーム状態。時間は UI では分で扱う（保存時に秒へ変換）。
  let workMin = $state(25);
  let breakMin = $state(5);
  let cyclesInfinite = $state(false);
  let cyclesCount = $state(0);
  let size = $state<settings.SizePreset>("medium");
  let corner = $state<settings.Corner>("topRight");
  let workEndSound = $state<SoundId>("chime");
  let breakEndSound = $state<SoundId>("ding");
  let sessionEndSound = $state<SoundId>("fanfare");
  let volume = $state(70);
  let focusBgm = $state<BgmId>("none");
  let bgmVolume = $state(40);
  let bgmPreviewing = $state(false);

  // ライブ反映（明示保存ではなく変更即適用）。「保存し忘れて閉じる」事故を構造的に無くす。
  // 数値入力の連打を避けるためデバウンスする。状態表示用に save の進行/エラーを持つ。
  type SaveState = "idle" | "saving" | "saved" | "error";
  let saveState = $state<SaveState>("idle");
  let errorMsg = $state("");

  // load 完了までは反映しない。load 直後の値で無駄保存（メインがリサイズで戻る等）を起こさないため、
  // 「最後に適用済みの内容」と一致する変更は無視する。
  let loaded = $state(false);
  let lastApplied = "";
  let debounceId: ReturnType<typeof setTimeout> | undefined;

  function num(v: number, fallback: number): number {
    return Number.isFinite(v) ? v : fallback;
  }

  function buildSettings(): settings.AppSettings {
    return {
      workSecs: settings.minutesToSecs(num(workMin, 25)),
      breakSecs: settings.minutesToSecs(num(breakMin, 5)),
      cyclesInfinite,
      cyclesCount: Math.max(0, Math.floor(num(cyclesCount, 0))),
      size,
      corner,
      workEndSound,
      breakEndSound,
      sessionEndSound,
      volume: Math.round(num(volume, 70)),
      focusBgm,
      bgmVolume: Math.round(num(bgmVolume, 40)),
    };
  }

  async function load() {
    try {
      const s = await settings.getSettings();
      workMin = settings.secsToMinutes(s.workSecs);
      breakMin = settings.secsToMinutes(s.breakSecs);
      cyclesInfinite = s.cyclesInfinite;
      cyclesCount = s.cyclesCount;
      size = s.size;
      corner = s.corner;
      workEndSound = s.workEndSound;
      breakEndSound = s.breakEndSound;
      sessionEndSound = s.sessionEndSound;
      volume = s.volume;
      focusBgm = s.focusBgm;
      bgmVolume = s.bgmVolume;
      lastApplied = JSON.stringify(buildSettings());
      loaded = true;
    } catch (e) {
      errorMsg = `Failed to load settings: ${e}`;
      saveState = "error";
    }
  }
  load();

  async function apply(payload: settings.AppSettings) {
    saveState = "saving";
    try {
      await settings.saveSettings(payload);
      lastApplied = JSON.stringify(payload);
      saveState = "saved";
    } catch (e) {
      errorMsg = `Failed to save: ${e}`;
      saveState = "error";
    }
  }

  // フォームの変化を監視し、デバウンスしてライブ反映する。
  $effect(() => {
    const payload = buildSettings();
    const json = JSON.stringify(payload);
    if (!loaded || json === lastApplied) return;
    clearTimeout(debounceId);
    debounceId = setTimeout(() => apply(payload), 350);
  });

  // BGM の試聴トグル。試聴中は選択/音量の変更に追従する（同一 BGM なら音量のみ更新）。
  function toggleBgmPreview() {
    if (bgmPreviewing) {
      stopBgm();
      bgmPreviewing = false;
    } else {
      bgmPreviewing = true; // $effect が実際の再生を行う
    }
  }
  $effect(() => {
    if (!bgmPreviewing) return;
    if (focusBgm === "none") {
      stopBgm();
      bgmPreviewing = false;
      return;
    }
    setBgm(focusBgm, bgmVolume / 100);
  });

  // 設定ウィンドウが非アクティブ（×で隠す/別ウィンドウへ）になったら試聴を止める
  // （Close ボタン以外の閉じ方でも鳴り続けないように）。
  getCurrentWindow().onFocusChanged(({ payload: focused }) => {
    if (!focused && bgmPreviewing) {
      stopBgm();
      bgmPreviewing = false;
    }
  });

  // conf 定義済みウィンドウなので破棄せず隠す（再度開ける）。試聴中なら止める。
  async function close() {
    stopBgm();
    bgmPreviewing = false;
    await hideWindow();
  }

  const statusText = $derived(
    saveState === "saving"
      ? "Saving…"
      : saveState === "saved"
        ? "✓ Saved"
        : saveState === "error"
          ? errorMsg
          : "Changes are saved automatically",
  );
</script>

<main>
  <h1>Settings</h1>

  <label class="row">
    <span>Focus (min)</span>
    <input
      type="number"
      min={settings.MIN_PHASE_MIN}
      max={settings.MAX_PHASE_MIN}
      bind:value={workMin}
    />
  </label>

  <label class="row">
    <span>Break (min)</span>
    <input
      type="number"
      min={settings.MIN_PHASE_MIN}
      max={settings.MAX_PHASE_MIN}
      bind:value={breakMin}
    />
  </label>

  <label class="row checkbox">
    <span>Loop forever</span>
    <input type="checkbox" bind:checked={cyclesInfinite} />
  </label>

  <label class="row">
    <span>Cycles (0 = stop after one)</span>
    <input
      type="number"
      min="0"
      max={settings.MAX_CYCLES}
      bind:value={cyclesCount}
      disabled={cyclesInfinite}
    />
  </label>

  <label class="row">
    <span>Position</span>
    <select bind:value={corner}>
      {#each settings.CORNER_OPTIONS as opt}
        <option value={opt.value}>{opt.label}</option>
      {/each}
    </select>
  </label>

  <label class="row">
    <span>Window size</span>
    <select bind:value={size}>
      {#each settings.SIZE_OPTIONS as opt}
        <option value={opt.value}>{opt.label}</option>
      {/each}
    </select>
  </label>

  <label class="row">
    <span>Focus end sound</span>
    <span class="sound">
      <select bind:value={workEndSound}>
        {#each settings.SOUND_OPTIONS as opt}
          <option value={opt.value}>{opt.label}</option>
        {/each}
      </select>
      <button
        class="preview"
        title="Preview"
        aria-label="Preview focus end sound"
        onclick={() => playSound(workEndSound, volume / 100)}>▶</button
      >
    </span>
  </label>

  <label class="row">
    <span>Break end sound</span>
    <span class="sound">
      <select bind:value={breakEndSound}>
        {#each settings.SOUND_OPTIONS as opt}
          <option value={opt.value}>{opt.label}</option>
        {/each}
      </select>
      <button
        class="preview"
        title="Preview"
        aria-label="Preview break end sound"
        onclick={() => playSound(breakEndSound, volume / 100)}>▶</button
      >
    </span>
  </label>

  <label class="row">
    <span>Session end sound</span>
    <span class="sound">
      <select bind:value={sessionEndSound}>
        {#each settings.SOUND_OPTIONS as opt}
          <option value={opt.value}>{opt.label}</option>
        {/each}
      </select>
      <button
        class="preview"
        title="Preview"
        aria-label="Preview session end sound"
        onclick={() => playSound(sessionEndSound, volume / 100)}>▶</button
      >
    </span>
  </label>

  <label class="row">
    <span>Volume ({volume})</span>
    <input
      type="range"
      min="0"
      max={settings.MAX_VOLUME}
      step="5"
      bind:value={volume}
    />
  </label>

  <hr />

  <label class="row">
    <span>Focus BGM (plays while focusing)</span>
    <span class="sound">
      <select bind:value={focusBgm}>
        {#each settings.BGM_OPTIONS as opt}
          <option value={opt.value}>{opt.label}</option>
        {/each}
      </select>
      <button
        class="preview"
        title={bgmPreviewing ? "Stop" : "Preview"}
        aria-label="Preview BGM"
        disabled={focusBgm === "none"}
        onclick={toggleBgmPreview}>{bgmPreviewing ? "■" : "▶"}</button
      >
    </span>
  </label>

  <label class="row">
    <span>BGM volume ({bgmVolume})</span>
    <input
      type="range"
      min="0"
      max={settings.MAX_VOLUME}
      step="5"
      bind:value={bgmVolume}
    />
  </label>

  <div class="footer">
    <span class="status" class:error={saveState === "error"}>{statusText}</span>
    <button onclick={close}>Close</button>
  </div>
</main>

<style>
  main {
    padding: 1rem 1.2rem;
    display: flex;
    flex-direction: column;
    gap: 0.7rem;
  }
  h1 {
    margin: 0 0 0.3rem;
    font-size: 1.1rem;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.8rem;
    font-size: 0.9rem;
  }
  .row span {
    flex: 1;
  }
  input[type="number"],
  select {
    width: 7rem;
    font: inherit;
    color: inherit;
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.18);
    border-radius: 5px;
    padding: 0.25rem 0.4rem;
  }
  .checkbox input {
    width: auto;
  }
  input[type="range"] {
    width: 7rem;
  }
  hr {
    width: 100%;
    border: none;
    border-top: 1px solid rgba(255, 255, 255, 0.12);
    margin: 0.2rem 0;
  }
  .sound {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    flex: none;
  }
  .sound select {
    width: 5.5rem;
  }
  .preview {
    font: inherit;
    color: inherit;
    background: rgba(255, 255, 255, 0.1);
    border: 1px solid rgba(255, 255, 255, 0.2);
    border-radius: 5px;
    padding: 0.2rem 0.45rem;
    cursor: pointer;
    line-height: 1;
  }
  .preview:hover {
    background: rgba(255, 255, 255, 0.18);
  }
  .footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    margin-top: 0.6rem;
  }
  .status {
    font-size: 0.78rem;
    opacity: 0.6;
  }
  .status.error {
    color: #e5736f;
    opacity: 1;
  }
  .footer button {
    font: inherit;
    color: inherit;
    background: rgba(255, 255, 255, 0.1);
    border: 1px solid rgba(255, 255, 255, 0.2);
    border-radius: 6px;
    padding: 0.35rem 0.9rem;
    cursor: pointer;
  }
  .footer button:hover {
    background: rgba(255, 255, 255, 0.18);
  }
</style>
