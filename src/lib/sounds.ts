// 通知音（#6）。音源ファイルは同梱せず、Web Audio API で短い合成音を生成する。
// 完全自作のためライセンス問題が無く、バイナリも増えない（CLAUDE.md の Don't / 軽量方針）。
//
// SoundId は src-tauri/src/settings.rs の SoundId enum（serde lowercase）と対応する。TS 側の正本は
// 本ファイル。音を1種増やすときは ① 下の SoundId 型 ② PLAYERS ③ settings.ts の SOUND_LABELS
// ④ settings.rs の SoundId enum の4箇所を同期する（SOUND_OPTIONS は SOUND_LABELS から自動生成で不要）。
import type { TimerEvent } from "./timer";

export type SoundId =
  | "none"
  | "beep"
  | "chime"
  | "ding"
  | "blip"
  | "fanfare";

/** 全音共通のゲイン上限。実音量は volume(0..1) を掛けて決める。 */
const PEAK_GAIN = 0.3;
/** アタック時間（クリックノイズ回避のため瞬間的に立ち上げない）。 */
const ATTACK_SECS = 0.01;
/** exponentialRamp は 0 を取れないため、無音とみなす下限値。 */
const SILENCE = 0.0001;
/** ノート停止までのマージン。 */
const TAIL_SECS = 0.02;

/** [周波数Hz, 開始秒(オフセット), 長さ秒] の並び。 */
type Note = [freq: number, start: number, dur: number];

let ctx: AudioContext | null = null;

function audioContext(): AudioContext {
  if (!ctx) ctx = new AudioContext();
  // ユーザー操作前は suspended のことがあるので毎回 resume を試みる（start 後なので通常は許可される）。
  if (ctx.state === "suspended") void ctx.resume();
  return ctx;
}

/** 正弦波 + 簡単なエンベロープでノート列を鳴らす（volume は 0..1）。 */
function playNotes(notes: Note[], volume: number): void {
  const ac = audioContext();
  const now = ac.currentTime;
  const peak = PEAK_GAIN * Math.max(0, Math.min(1, volume));
  if (peak <= 0) return;
  for (const [freq, start, dur] of notes) {
    const osc = ac.createOscillator();
    const gain = ac.createGain();
    osc.type = "sine";
    osc.frequency.value = freq;
    const t0 = now + start;
    const t1 = t0 + dur;
    gain.gain.setValueAtTime(0, t0);
    gain.gain.linearRampToValueAtTime(peak, t0 + ATTACK_SECS);
    gain.gain.exponentialRampToValueAtTime(SILENCE, t1);
    osc.connect(gain).connect(ac.destination);
    osc.start(t0);
    osc.stop(t1 + TAIL_SECS);
  }
}

// 各プリセットの合成内容。"none" は鳴らさないので含めない。
const PLAYERS: Record<Exclude<SoundId, "none">, (volume: number) => void> = {
  beep: (v) => playNotes([[880, 0, 0.15]], v),
  chime: (v) => playNotes([[659.25, 0, 0.13], [880, 0.13, 0.2]], v),
  ding: (v) => playNotes([[1174.66, 0, 0.35]], v),
  blip: (v) => playNotes([[523.25, 0, 0.07], [784, 0.08, 0.12]], v),
  // 完走を知らせる上昇 3 音（C-E-G）。
  fanfare: (v) =>
    playNotes([[523.25, 0, 0.12], [659.25, 0.12, 0.12], [784, 0.24, 0.28]], v),
};

/** 指定の通知音を鳴らす。"none" や未知の id は無音（壊れた設定でも安全）。volume は 0..1。 */
export function playSound(id: SoundId, volume = 1): void {
  const player = id === "none" ? undefined : PLAYERS[id];
  if (!player) return;
  try {
    player(volume);
  } catch (e) {
    // 音が出せない環境でもタイマー本体は止めない。
    console.error("failed to play sound", e);
  }
}

/** 通知音の選択（フェーズ別）。 */
export interface SoundChoices {
  work: SoundId;
  break: SoundId;
  session: SoundId;
}

/**
 * 1 回の tick で届いたイベント列に対して、鳴らすべき音を順に返す純粋関数。
 *
 * - セッション完了が含まれる場合は完了音のみ（最終休憩終了音より完了を優先）。
 * - それ以外は、含まれていれば作業終了音・休憩終了音をそれぞれ 1 回ずつ。
 * - スリープ復帰などで同種イベントが複数来ても各 1 回に畳み、連打しない。
 */
export function soundsForEvents(
  events: TimerEvent[],
  choices: SoundChoices,
): SoundId[] {
  if (events.includes("sessionFinished")) return [choices.session];
  const out: SoundId[] = [];
  if (events.includes("workEnded")) out.push(choices.work);
  if (events.includes("breakEnded")) out.push(choices.break);
  return out;
}
