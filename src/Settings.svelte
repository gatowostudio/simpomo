<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { isEnabled, enable, disable } from "@tauri-apps/plugin-autostart";
  import * as settings from "./lib/settings";
  import { hideWindow } from "./lib/window";
  import { playSound, type SoundId } from "./lib/sounds";
  import { setBgm, stopBgm, type BgmId } from "./lib/bgm";
  import { ensureNotificationPermission } from "./lib/notify";
  import { getStats, resetStats, onStatsChanged, type Stats } from "./lib/stats";
  import { checkForUpdate, openReleases } from "./lib/update";

  // 編集中のフォーム状態。時間は UI では分で扱う（保存時に秒へ変換）。
  let workMin = $state(25);
  let breakMin = $state(5);
  let cyclesInfinite = $state(false);
  let cyclesCount = $state(0);
  let corner = $state<settings.Corner>("topRight");
  let skipTaskbar = $state(true);
  let autostartTimer = $state(false);
  // OS スタートアップ登録は OS 側が真実。settings.json には持たず autostart プラグインで読み書きする。
  let launchOnStartup = $state(false);
  let workEndSound = $state<SoundId>("chime");
  let breakEndSound = $state<SoundId>("ding");
  let sessionEndSound = $state<SoundId>("fanfare");
  let volume = $state(70);
  let osNotifications = $state(true);
  let focusBgm = $state<BgmId>("none");
  let bgmVolume = $state(25);
  let bgmPreviewing = $state(false);
  let focusBgColor = $state(settings.DEFAULT_FOCUS_BG);
  let breakBgColor = $state(settings.DEFAULT_BREAK_BG);
  // 完了数の統計（#22）。集計は Rust 権威で、ここは表示とリセットのみ。
  let stats = $state<Stats>({ completedFocus: 0, completedSets: 0 });

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
      corner,
      skipTaskbar,
      autostartTimer,
      workEndSound,
      breakEndSound,
      sessionEndSound,
      volume: Math.round(num(volume, 70)),
      osNotifications,
      focusBgm,
      bgmVolume: Math.round(num(bgmVolume, 25)),
      focusBgColor,
      breakBgColor,
    };
  }

  async function load() {
    try {
      const s = await settings.getSettings();
      workMin = settings.secsToMinutes(s.workSecs);
      breakMin = settings.secsToMinutes(s.breakSecs);
      cyclesInfinite = s.cyclesInfinite;
      cyclesCount = s.cyclesCount;
      corner = s.corner;
      skipTaskbar = s.skipTaskbar;
      autostartTimer = s.autostartTimer;
      workEndSound = s.workEndSound;
      breakEndSound = s.breakEndSound;
      sessionEndSound = s.sessionEndSound;
      volume = s.volume;
      osNotifications = s.osNotifications;
      focusBgm = s.focusBgm;
      bgmVolume = s.bgmVolume;
      focusBgColor = s.focusBgColor;
      breakBgColor = s.breakBgColor;
      lastApplied = JSON.stringify(buildSettings());
      loaded = true;
      // OS スタートアップ登録の現在状態は OS 側に問い合わせる（settings.json とは別管理）。
      launchOnStartup = await isEnabled().catch(() => false);
      // 完了数の統計を取得（#22）。失敗しても他の設定表示は止めない。
      stats = await getStats().catch(() => stats);
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

  // OS スタートアップ登録のトグル（#21）。これは settings.json ではなく OS 側へ即時反映するため、
  // 上の自動保存フローには乗せず、ここで autostart プラグインを直接呼ぶ。失敗時は実状態へ戻す。
  async function setLaunchOnStartup(on: boolean) {
    try {
      if (on) await enable();
      else await disable();
      launchOnStartup = on;
      saveState = "saved";
    } catch (e) {
      errorMsg = `Failed to update startup: ${e}`;
      saveState = "error";
      launchOnStartup = await isEnabled().catch(() => launchOnStartup);
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

  // 更新確認（手動）。押したときだけ GitHub Releases（公開版）を見る。
  type UpdateState = "idle" | "checking" | "upToDate" | "available" | "error";
  const UPDATE_HINT = "Check GitHub for a newer version";
  let updateState = $state<UpdateState>("idle");
  let updateText = $state(UPDATE_HINT);
  async function checkUpdate() {
    updateState = "checking";
    updateText = "Checking…";
    try {
      const info = await checkForUpdate();
      if (info.newer) {
        updateState = "available";
        updateText = `New version ${info.latest} available (you have ${info.current})`;
      } else if (info.latest === "") {
        updateState = "upToDate";
        updateText = `No published releases yet (you have ${info.current})`;
      } else {
        updateState = "upToDate";
        updateText = `Up to date (${info.current})`;
      }
    } catch (e) {
      updateState = "error";
      updateText = e instanceof Error ? e.message : String(e);
    }
  }

  // 設定ウィンドウが非アクティブ（×で隠す/別ウィンドウへ）になったら試聴を止める
  // （Close ボタン以外の閉じ方でも鳴り続けないように）。
  getCurrentWindow().onFocusChanged(({ payload: focused }) => {
    if (!focused && bgmPreviewing) {
      stopBgm();
      bgmPreviewing = false;
    }
    // 再表示のたびに更新確認の表示はまっさらに戻す（前回結果を引きずらない）。
    if (focused) {
      updateState = "idle";
      updateText = UPDATE_HINT;
      // 隠れている間に進んだ完了数を表示へ反映する（#22）。
      getStats()
        .then((s) => (stats = s))
        .catch(() => {});
    }
  });

  // 表示中に境界へ到達したらライブ更新する。設定ウィンドウは conf 定義の常駐窓で破棄されず
  // （閉じる=hide）、本スクリプトは一度だけ実行される＝購読は 1 回・解除不要（上の onFocusChanged と同流儀）。
  onStatsChanged((s) => (stats = s));

  // 完了数のリセット（ユーザー操作）。Rust 側で 0 にして永続化し、stats-changed で戻ってくる。
  async function resetStatsClick() {
    try {
      await resetStats();
    } catch (e) {
      errorMsg = `Failed to reset stats: ${e}`;
      saveState = "error";
    }
  }

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

  <label
    class="row"
    title="Where the window first appears. Pick a corner to move it there now; afterwards your dragged position is remembered."
  >
    <span>Position (initial / reset)</span>
    <select bind:value={corner}>
      {#each settings.CORNER_OPTIONS as opt}
        <option value={opt.value}>{opt.label}</option>
      {/each}
    </select>
  </label>

  <label class="row checkbox">
    <span>Tray only (hide from taskbar)</span>
    <input type="checkbox" bind:checked={skipTaskbar} />
  </label>

  <label class="row checkbox">
    <span>Auto-start timer on launch</span>
    <input type="checkbox" bind:checked={autostartTimer} />
  </label>

  <label class="row checkbox">
    <span>Launch on system startup</span>
    <!-- これは settings.json でなく OS 側へ即時反映するため bind せず onchange で直接呼ぶ（setLaunchOnStartup 参照）。 -->
    <input
      type="checkbox"
      checked={launchOnStartup}
      onchange={(e) => setLaunchOnStartup(e.currentTarget.checked)}
    />
  </label>

  <p class="hint">
    Auto-started sounds may stay silent until you interact (enable notifications
    below for hands-free alerts). “Launch on system startup” is saved by the OS,
    not in settings.
  </p>

  <label class="row">
    <span>Focus background</span>
    <input type="color" bind:value={focusBgColor} />
  </label>

  <label class="row">
    <span>Break background</span>
    <input type="color" bind:value={breakBgColor} />
  </label>

  <p class="hint">
    Drag the window to move it, drag an edge to resize. Position and size are
    remembered.
  </p>

  <hr />

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

  <label class="row checkbox">
    <span>Notify when hidden (tray)</span>
    <!-- ON にした瞬間（=可視・意図的な操作）に権限を要求する。隠れている最中には要求しない。 -->
    <input
      type="checkbox"
      bind:checked={osNotifications}
      onchange={(e) => e.currentTarget.checked && ensureNotificationPermission()}
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

  <hr />

  <div class="row">
    <span>Completed (focus / sets)</span>
    <span class="sound">
      <span class="stat">{stats.completedFocus} / {stats.completedSets}</span>
      <button class="preview" title="Reset stats" onclick={resetStatsClick}>Reset</button>
    </span>
  </div>

  <hr />

  <div class="row">
    <span class="update" class:error={updateState === "error"}>{updateText}</span>
    <span class="sound">
      {#if updateState === "available"}
        <button class="preview" title="Open releases" onclick={() => openReleases()}
          >Open</button
        >
      {/if}
      <button class="preview" onclick={checkUpdate} disabled={updateState === "checking"}
        >Check</button
      >
    </span>
  </div>

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
  input[type="color"] {
    width: 2.5rem;
    height: 1.6rem;
    padding: 0;
    border: 1px solid rgba(255, 255, 255, 0.25);
    border-radius: 5px;
    background: none;
    cursor: pointer;
  }
  .hint {
    font-size: 0.75rem;
    opacity: 0.5;
    margin: 0.1rem 0 0;
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
  .status,
  .update {
    font-size: 0.78rem;
    opacity: 0.6;
  }
  .stat {
    font-variant-numeric: tabular-nums;
    opacity: 0.8;
  }
  .status.error,
  .update.error {
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
