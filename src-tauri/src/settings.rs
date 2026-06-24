//! アプリ設定の永続化（#5）。
//!
//! 設定の「真実」はアプリ設定ディレクトリの `settings.json`（ADR-0002）。起動時に読み込んで
//! タイマーとウィンドウに適用し、設定ウィンドウが保存すると本ファイルを更新して反映する。
//! store プラグインは入れず、軽量に serde_json でファイル入出力する。
//!
//! 値の検証は Rust が権威（ADR-0002）: `sanitized()` が下限・上限・整合を保証する純粋関数で、
//! 保存前・適用前に必ず通す。フロントの clamp は UX 用であって信頼境界ではない。

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::layout::{Corner, SizePreset};
use crate::timer::{Config, CycleSetting};

/// フェーズ時間の下限（1 分）・上限（180 分）。設定 UI も同じ範囲を提示する。
pub const MIN_PHASE_SECS: u32 = 60;
pub const MAX_PHASE_SECS: u32 = 180 * 60;
/// サイクル数の上限。
pub const MAX_CYCLES: u32 = 99;

/// 永続化するアプリ設定。フロント（src/lib/settings.ts）の手書きミラーと camelCase で対応する。
///
/// `#[serde(default)]`: 将来フィールドが増えても（#6 の通知音など）、旧 `settings.json` に
/// 欠けたフィールドは `Default` から補われる。これが無いと欠損フィールドで deserialize が失敗し、
/// `load` のフォールバックで全設定が既定に戻ってしまう。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// 作業フェーズ秒数（UI では分で扱う）。
    pub work_secs: u32,
    /// 休憩フェーズ秒数。
    pub break_secs: u32,
    /// 無限に繰り返すか。true のとき `cycles_count` は無視され、保存時 0 に正規化される。
    pub cycles_infinite: bool,
    /// 自動継続するセット数。0 = 1 セットで停止（既定）。`cycles_infinite` が false のとき有効。
    pub cycles_count: u32,
    pub size: SizePreset,
    pub corner: Corner,
}

impl Default for AppSettings {
    fn default() -> Self {
        let c = Config::default();
        Self {
            work_secs: c.work_secs,
            break_secs: c.break_secs,
            cycles_infinite: false,
            cycles_count: 0,
            size: SizePreset::Medium,
            corner: Corner::TopRight,
        }
    }
}

impl AppSettings {
    /// 値域を保証し整合を取った設定を返す（Rust が値検証の権威）。
    /// 保存前・適用前に必ず通す。フロントを信頼しないための不変条件。
    pub fn sanitized(self) -> Self {
        Self {
            work_secs: self.work_secs.clamp(MIN_PHASE_SECS, MAX_PHASE_SECS),
            break_secs: self.break_secs.clamp(MIN_PHASE_SECS, MAX_PHASE_SECS),
            cycles_infinite: self.cycles_infinite,
            // 無限のときは回数を 0 に正規化し、無意味な値を永続化しない。
            cycles_count: if self.cycles_infinite {
                0
            } else {
                self.cycles_count.min(MAX_CYCLES)
            },
            size: self.size,
            corner: self.corner,
        }
    }

    /// タイマーのコア設定へ変換する。
    pub fn to_config(self) -> Config {
        Config {
            work_secs: self.work_secs,
            break_secs: self.break_secs,
            cycles: if self.cycles_infinite {
                CycleSetting::Infinite
            } else {
                CycleSetting::Finite(self.cycles_count)
            },
        }
    }
}

fn settings_path(app: &AppHandle) -> tauri::Result<PathBuf> {
    Ok(app.path().app_config_dir()?.join("settings.json"))
}

/// 設定を読み込む。ファイルが無い / 壊れている場合は既定値を返す（起動を止めない）。
pub fn load(app: &AppHandle) -> AppSettings {
    settings_path(app)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 設定を `settings.json` に保存する。
pub fn save(app: &AppHandle, settings: &AppSettings) -> Result<(), String> {
    let path = settings_path(app).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_25_5_and_finite_zero() {
        let s = AppSettings::default();
        assert_eq!(s.work_secs, 1500);
        assert_eq!(s.break_secs, 300);
        assert!(!s.cycles_infinite);
        assert_eq!(s.cycles_count, 0);
        assert_eq!(s.size, SizePreset::Medium);
        assert_eq!(s.corner, Corner::TopRight);
    }

    #[test]
    fn to_config_maps_finite_cycles() {
        let s = AppSettings {
            cycles_infinite: false,
            cycles_count: 4,
            ..Default::default()
        };
        assert_eq!(s.to_config().cycles, CycleSetting::Finite(4));
    }

    #[test]
    fn to_config_infinite_ignores_count() {
        let s = AppSettings {
            cycles_infinite: true,
            cycles_count: 9,
            ..Default::default()
        };
        assert_eq!(s.to_config().cycles, CycleSetting::Infinite);
    }

    #[test]
    fn sanitized_clamps_phase_and_normalizes_infinite() {
        let s = AppSettings {
            work_secs: 0,
            break_secs: 999_999,
            cycles_infinite: true,
            cycles_count: 50,
            ..Default::default()
        }
        .sanitized();
        assert_eq!(s.work_secs, MIN_PHASE_SECS);
        assert_eq!(s.break_secs, MAX_PHASE_SECS);
        assert_eq!(s.cycles_count, 0); // 無限のとき 0 に正規化
    }

    #[test]
    fn sanitized_caps_cycles_count() {
        let s = AppSettings {
            cycles_infinite: false,
            cycles_count: 1000,
            ..Default::default()
        }
        .sanitized();
        assert_eq!(s.cycles_count, MAX_CYCLES);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults_not_full_reset() {
        // #6 でフィールドが増えても、旧 JSON(一部欠損)が全既定化しないことを固定する。
        let json = r#"{"workSecs": 1800}"#;
        let s: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.work_secs, 1800); // 指定値は保持
        assert_eq!(s.break_secs, 300); // 欠損は既定
        assert_eq!(s.size, SizePreset::Medium); // 欠損は既定
    }

    #[test]
    fn json_round_trips_with_camel_case() {
        // フロント settings.ts との camelCase 契約を固定する。
        let s = AppSettings {
            work_secs: 1800,
            break_secs: 600,
            cycles_infinite: true,
            cycles_count: 0,
            size: SizePreset::Large,
            corner: Corner::BottomLeft,
        };
        let json = serde_json::to_string(&s).unwrap();
        for key in [
            "workSecs",
            "breakSecs",
            "cyclesInfinite",
            "cyclesCount",
            "size",
            "corner",
        ] {
            assert!(json.contains(&format!("\"{key}\"")), "missing key: {key}");
        }
        assert!(json.contains("\"bottomLeft\""));
        let back: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }
}
