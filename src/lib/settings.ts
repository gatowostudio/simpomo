// 設定の永続化（#5）への薄いブリッジ。
//
// 型の同期契約: 下記の型・enum 文字列は src-tauri/src/settings.rs（AppSettings）と
// src-tauri/src/layout.rs（SizePreset/Corner）の手書きミラー。Rust 側のフィールド名・serde rename
// （camelCase / lowercase）・enum の variant を変えたら本ファイルも必ず同期すること。
// ドリフトは Rust 側テスト（settings.rs: json_round_trips / missing_fields_*、layout.rs: *_deserializes_*）が
// 部分的に検出する。型生成（tauri-specta 等）は未導入で手動同期。
// ※ SoundId は TS 側の正本を sounds.ts とし、ここは re-export しているだけ（音の追加手順は sounds.ts 参照）。
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { SoundId } from "./sounds";

export type { SoundId };

/** 設定ウィンドウのラベル。Rust(open_settings) と main.ts の出し分けで一致させる文字列契約。 */
export const SETTINGS_WINDOW_LABEL = "settings";

/** 設定変更イベント名。Rust(lib.rs EVENT_SETTINGS) と一致させる。 */
const EVENT_SETTINGS_CHANGED = "settings-changed";

// 時間換算・値域の定数（Rust settings.rs の MIN/MAX_PHASE_SECS, MAX_CYCLES と対応させる）。
export const SECS_PER_MINUTE = 60;
export const MIN_PHASE_MIN = 1;
export const MAX_PHASE_MIN = 180;
export const MAX_CYCLES = 99;

export const minutesToSecs = (min: number): number =>
  Math.round(min) * SECS_PER_MINUTE;
export const secsToMinutes = (secs: number): number =>
  Math.round(secs / SECS_PER_MINUTE);

export type SizePreset =
  | "xsmall"
  | "small"
  | "medium"
  | "large"
  | "xlarge"
  | "xxlarge";

export type Corner = "topRight" | "topLeft" | "bottomRight" | "bottomLeft";

export interface AppSettings {
  workSecs: number;
  breakSecs: number;
  cyclesInfinite: boolean;
  /** 0 = 1 セットで停止（既定）。cyclesInfinite が false のとき有効。 */
  cyclesCount: number;
  size: SizePreset;
  corner: Corner;
  workEndSound: SoundId;
  breakEndSound: SoundId;
  sessionEndSound: SoundId;
  /** 通知音の音量（0〜100）。 */
  volume: number;
}

export const getSettings = (): Promise<AppSettings> => invoke("get_settings");

export const saveSettings = (settings: AppSettings): Promise<void> =>
  invoke("save_settings", { settings });

/** 設定ウィンドウを開く（メイン画面の歯車ボタンから）。 */
export const openSettings = (): Promise<void> => invoke("open_settings");

/** 設定変更の通知を購読する（#6 の通知音などが使う）。 */
export const onSettingsChanged = (
  cb: (settings: AppSettings) => void,
): Promise<UnlistenFn> =>
  listen<AppSettings>(EVENT_SETTINGS_CHANGED, (e) => cb(e.payload));

// ラベルは Record で全 variant の網羅を型強制する（layout.rs の enum に variant を足したら
// ここがコンパイルエラーになり、同期漏れを防ぐ）。表示順は定義順。
const SIZE_LABELS: Record<SizePreset, string> = {
  xsmall: "XS",
  small: "S",
  medium: "M",
  large: "L",
  xlarge: "XL",
  xxlarge: "XXL",
};
const CORNER_LABELS: Record<Corner, string> = {
  topRight: "Top right",
  topLeft: "Top left",
  bottomRight: "Bottom right",
  bottomLeft: "Bottom left",
};

const SOUND_LABELS: Record<SoundId, string> = {
  none: "None",
  beep: "Beep",
  chime: "Chime",
  ding: "Ding",
  blip: "Blip",
  fanfare: "Fanfare",
};

/** 音量の上限（Rust settings.rs MAX_VOLUME と対応）。 */
export const MAX_VOLUME = 100;

export const SIZE_OPTIONS = (Object.keys(SIZE_LABELS) as SizePreset[]).map(
  (value) => ({ value, label: SIZE_LABELS[value] }),
);
export const CORNER_OPTIONS = (Object.keys(CORNER_LABELS) as Corner[]).map(
  (value) => ({ value, label: CORNER_LABELS[value] }),
);
export const SOUND_OPTIONS = (Object.keys(SOUND_LABELS) as SoundId[]).map(
  (value) => ({ value, label: SOUND_LABELS[value] }),
);
