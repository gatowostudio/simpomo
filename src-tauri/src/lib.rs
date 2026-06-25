pub mod layout;
pub mod settings;
pub mod stats;
pub mod timer;

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

use layout::Corner;
use settings::AppSettings;
use stats::Stats;

use timer::{Config, Status, Timer, TimerEvent, TimerSnapshot};

/// タイマー状態と、tick 駆動スレッドを起こすための条件変数、完了数の統計をまとめた共有状態。
///
/// 並行性の不変条件: 各 Mutex を保持している間は panic しうる処理を呼ばない
/// （Timer/Stats 操作は純粋で panic せず、save はエラーをログして握りつぶす）。よって Mutex は
/// poison せず、`lock().unwrap()` は安全。`timer` と `stats` を**同時に保持する経路は無い**
/// （tick スレッドは stats を触る前に timer ロックを手放す）ので、ロック順による deadlock は生じない。
struct Shared {
    timer: Mutex<Timer>,
    /// 稼働状態の変化を tick スレッドへ通知する。start で park 中のスレッドを起こす。
    wake: Condvar,
    /// 完了数の簡易統計（#22）。tick の境界イベントで増え、stats.json に永続化する。
    stats: Mutex<Stats>,
}

type SharedState = Arc<Shared>;

// イベント名はフロント（timer.ts / settings.ts）と一致させる文字列契約。
// 単純な英数字 + ハイフンにする（":" や "/" を含む名前はトラブルの元になりうる）。
const EVENT_SNAPSHOT: &str = "timer-snapshot";
const EVENT_TIMER_EVENTS: &str = "timer-events";
/// 設定変更を全ウィンドウへ通知する（snapshot に乗らない設定を #6 等が購読する）。
const EVENT_SETTINGS: &str = "settings-changed";
/// 完了数の統計更新を通知する（#22。設定ウィンドウが購読してライブ表示する）。
const EVENT_STATS: &str = "stats-changed";

/// ウィンドウラベル。フロント（main.ts / settings.ts）と一致させる文字列契約。
const MAIN_LABEL: &str = "main";
const SETTINGS_LABEL: &str = "settings";

// snapshot / フェーズ境界イベントはブロードキャストで送る（フロントの listen() に確実に届く）。
// 設定ウィンドウはこれらを購読しないので二重処理にはならない。
fn emit_snapshot(app: &AppHandle, snapshot: TimerSnapshot) {
    // 送信失敗（ウィンドウ破棄直後など）は致命ではないので握りつぶす。
    let _ = app.emit(EVENT_SNAPSHOT, snapshot);
}

fn emit_events(app: &AppHandle, events: &[TimerEvent]) {
    if !events.is_empty() {
        let _ = app.emit(EVENT_TIMER_EVENTS, events);
    }
}

/// フェーズ境界イベントを完了数の統計へ反映し、変化があれば永続化して通知する（#22）。
/// `timer` ロックを手放した状態で呼ぶ（呼び出し側が drop 済み）。`stats` ロックは集計の一瞬だけ
/// 保持してコピーを取り、ファイル I/O と emit はロック外で行う（ボタン操作を I/O で待たせない）。
fn record_stats(app: &AppHandle, state: &SharedState, events: &[TimerEvent]) {
    let updated = {
        let mut stats = state.stats.lock().unwrap();
        stats.record(events).then_some(*stats)
    };
    if let Some(stats) = updated {
        // 保存失敗（ディスク満杯/権限等）は致命ではないが、表示と永続が乖離するのでログは残す。
        if let Err(e) = stats::save(app, &stats) {
            eprintln!("simpomo: failed to persist stats: {e}");
        }
        let _ = app.emit(EVENT_STATS, stats);
    }
}

// コマンドは状態を「戻り値」では返さない。更新は必ず emit(EVENT_SNAPSHOT) の単一経路で流す
// （戻り値と emit の二重経路だと到着順でフロント表示が巻き戻るレースが起きるため）。
// mutate と emit は同一ロック区間で行い、tick スレッドの emit と順序が入れ替わらないようにする。

#[tauri::command]
fn timer_snapshot(state: State<'_, SharedState>) -> TimerSnapshot {
    state.timer.lock().unwrap().snapshot()
}

#[tauri::command]
fn timer_start(app: AppHandle, state: State<'_, SharedState>) {
    {
        let mut timer = state.timer.lock().unwrap();
        timer.start();
        emit_snapshot(&app, timer.snapshot());
    }
    state.wake.notify_all(); // park 中の tick スレッドを起こす
}

#[tauri::command]
fn timer_pause(app: AppHandle, state: State<'_, SharedState>) {
    {
        let mut timer = state.timer.lock().unwrap();
        timer.pause();
        emit_snapshot(&app, timer.snapshot());
    }
    state.wake.notify_all();
}

#[tauri::command]
fn timer_reset(app: AppHandle, state: State<'_, SharedState>) {
    {
        let mut timer = state.timer.lock().unwrap();
        timer.reset();
        emit_snapshot(&app, timer.snapshot());
    }
    state.wake.notify_all();
}

#[tauri::command]
fn timer_skip(app: AppHandle, state: State<'_, SharedState>) {
    {
        let mut timer = state.timer.lock().unwrap();
        // 手動 skip はフェーズ境界イベントを出さない（自分で送ったのに通知音が鳴る違和感を避ける、殿の判断）。
        // 状態変化は snapshot で反映する。時間切れの遷移（tick）だけが通知音を鳴らす。
        let _ = timer.skip();
        emit_snapshot(&app, timer.snapshot());
    }
    state.wake.notify_all();
}

/// メインウィンドウを指定の隅へ移動する（設定保存・起動時の共通経路）。サイズは触らない。
fn apply_main_position(app: &AppHandle, corner: Corner) -> Result<(), String> {
    let main = app
        .get_webview_window(MAIN_LABEL)
        .ok_or_else(|| "main window not found".to_string())?;
    layout::apply_position(&main, corner).map_err(|e| e.to_string())
}

/// メインをタスクバーに出すか（false）/ トレイのみ（true）を適用する。
fn apply_skip_taskbar(app: &AppHandle, skip: bool) {
    if let Some(main) = app.get_webview_window(MAIN_LABEL) {
        let _ = main.set_skip_taskbar(skip);
    }
}

/// window-state プラグインが状態ファイルを書いたことがあるか＝「初回起動ではない」かを返す（#16）。
///
/// 「初回だけ既定の隅へ、以降は復元位置を尊重する」を、保存された座標の**中身を見ずにファイルの
/// 存在だけ**で判定する。座標値（特に最大化中に保存されると x/y が (0,0) に化ける／ユーザーが偶然
/// 左上に置いて (0,0) になる等）を覗くと初回判定を誤るため、存在判定に寄せて誤検出を断つ。
/// 保存先はプラグインと同じ app_config_dir/ファイル名（filename() はプラグインの公開 API）。
fn window_state_saved(app: &AppHandle) -> bool {
    app.path()
        .app_config_dir()
        .map(|dir| dir.join(app.filename()).exists())
        .unwrap_or(false)
}

/// 現在の永続化設定を返す（設定ウィンドウの初期表示用）。
#[tauri::command]
fn get_settings(app: AppHandle) -> AppSettings {
    settings::load(&app)
}

/// 完了数の統計を返す（設定ウィンドウの表示用、#22）。
#[tauri::command]
fn get_stats(state: State<'_, SharedState>) -> Stats {
    *state.stats.lock().unwrap()
}

/// 完了数の統計を 0 に戻す（設定ウィンドウの Reset から。ユーザー操作で実行、#22）。
#[tauri::command]
fn reset_stats(app: AppHandle, state: State<'_, SharedState>) -> Result<(), String> {
    let stats = {
        let mut s = state.stats.lock().unwrap();
        *s = Stats::default();
        *s
    };
    stats::save(&app, &stats)?;
    let _ = app.emit(EVENT_STATS, stats);
    Ok(())
}

/// 設定を保存し、タイマーとウィンドウへ反映する（設定ウィンドウから呼ぶ）。
#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: State<'_, SharedState>,
    settings: AppSettings,
) -> Result<(), String> {
    // Position 設定（隅）が実際に変わったかを、保存で上書きする前の値と比べて判定する（#16）。
    let previous_corner = settings::load(&app).corner;
    // Rust が値検証の権威。保存値=実効値になるよう先に正規化する。
    let settings = settings.sanitized();
    settings::save(&app, &settings)?;
    // タイマーへ反映（Idle なら新時間で reset、稼働中はセッション維持し次フェーズ以降）。
    {
        let mut timer = state.timer.lock().unwrap();
        timer.set_config(settings.to_config());
        emit_snapshot(&app, timer.snapshot());
    }
    state.wake.notify_all();
    let corner = settings.corner;
    let corner_changed = corner != previous_corner;
    apply_skip_taskbar(&app, settings.skip_taskbar);
    // 設定変更を通知（snapshot に乗らない設定＝通知音/BGM/背景色を App が購読して反映）。
    let _ = app.emit(EVENT_SETTINGS, settings);
    // Position は「初期/リセット位置」。隅を変更したときだけその隅へ寄せる。それ以外の設定変更では
    // ユーザーがドラッグで置いた現在位置を尊重する（#16。サイズ/位置は plugin が永続化）。
    if corner_changed {
        apply_main_position(&app, corner)?;
    }
    Ok(())
}

/// 設定ウィンドウを表示する（conf で定義済みの非表示ウィンドウを見せる）。別ウィンドウ方式（ADR-0002）。
#[tauri::command]
fn open_settings(app: AppHandle) -> Result<(), String> {
    let win = app
        .get_webview_window(SETTINGS_LABEL)
        .ok_or_else(|| "settings window not found".to_string())?;
    win.show().map_err(|e| e.to_string())?;
    let _ = win.unminimize();
    let _ = win.set_focus();
    Ok(())
}

/// 呼び出し元ウィンドウの最前面表示を切り替える（メインのピンボタンから）。
/// JS の window API ではなく Rust から直接制御し、確実に効くようにする。
#[tauri::command]
fn set_always_on_top(window: tauri::WebviewWindow, on: bool) -> Result<(), String> {
    window.set_always_on_top(on).map_err(|e| e.to_string())
}

/// 呼び出し元ウィンドウを隠す（メインの ✕ / 設定の Close から）。トレイ常駐のため終了はしない。
#[tauri::command]
fn hide_window(window: tauri::WebviewWindow) -> Result<(), String> {
    window.hide().map_err(|e| e.to_string())
}

/// メインウィンドウを表示して前面に出す（トレイ / 二重起動の検知から呼ぶ）。
fn show_main(app: &AppHandle) {
    if let Some(main) = app.get_webview_window(MAIN_LABEL) {
        let _ = main.show();
        let _ = main.unminimize();
        let _ = main.set_focus();
    }
}

/// 起動時に永続化設定を読み込み、タイマーとウィンドウへ適用してメインを表示する。
fn apply_loaded_settings(app: &AppHandle, state: &SharedState) {
    let loaded = settings::load(app);
    // 完了数の統計を stats.json から読み直す（#22）。
    *state.stats.lock().unwrap() = stats::load(app);
    {
        let mut timer = state.timer.lock().unwrap();
        timer.set_config(loaded.to_config());
        // 起動時に自動でタイマーを開始するオプション（#21）。tick ループはこの後 spawn され、
        // Running 状態を見てそのまま計時を始める（park されない）。
        if loaded.autostart_timer {
            timer.start();
        }
    }
    // ウィンドウは conf で visible:false。サイズ/位置は window-state プラグインが復元済み（#16）。
    // タスクバー表示と位置を確定してから表示し、チラつき（タスクバーボタンの一瞬の出現/位置ジャンプ）を避ける。
    apply_skip_taskbar(app, loaded.skip_taskbar);
    if let Some(main) = app.get_webview_window(MAIN_LABEL) {
        // 初回起動（状態ファイルなし）は既定の隅へ。以降はプラグインが復元した位置を尊重しつつ、
        // 解像度/スケール変更やモニタ取り外しで画面外に出ていれば作業領域へ救済する（#16/#17）。
        // ※プラグインの復元は on_window_ready で本 setup より前に走るため、ここでは復元済み前提でよい。
        if window_state_saved(app) {
            let _ = layout::ensure_on_screen(&main);
        } else {
            let _ = layout::apply_position(&main, loaded.corner);
        }
        let _ = main.show();
    }
}

/// システムトレイを構築する。左クリックでウィンドウ表示、メニューで表示/終了（トレイ常駐）。
fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

    let mut builder = TrayIconBuilder::with_id("main").tooltip("simpomo");
    // アイコンは conf で定義済みだが、念のため取得できたときだけ設定する（panic を避ける）。
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// tick の間隔は通常 1 秒（下記ループの wait_timeout）。これを大きく超える経過は
/// スリープ/サスペンドからの復帰を意味し、その間ユーザーは作業していない。統計(#22)は
/// この値を超える catch-up バッチを実績に数えない（過小評価側に倒す＝水増ししない）。
const STATS_MAX_LIVE_GAP_SECS: u32 = 5;

/// 実時間の駆動（ADR-0002）。Rust 側のバックグラウンドスレッドが単調時計（`Instant`）の
/// 差分で実経過秒を算出して `tick` に渡す。固定 `tick(1)` のドリフトとスリープ復帰のずれを避ける。
///
/// アイドル時 CPU を最小化するため、非稼働中（Idle/Paused）は条件変数で park し、
/// CPU を消費しない。start の `notify_all` で起き、稼働中のみ毎秒 tick して emit する。
fn spawn_tick_loop(app: AppHandle, state: SharedState) {
    std::thread::spawn(move || {
        let mut last = Instant::now();
        let mut timer = state.timer.lock().unwrap();
        loop {
            // 非稼働中は wake されるまで park（この間ロックは解放され CPU はゼロ）。
            while timer.snapshot().status != Status::Running {
                timer = state.wake.wait(timer).unwrap();
                // 稼働再開の起点を切り直す（park 中の経過を取り込んで過剰進行するのを防ぐ）。
                last = Instant::now();
            }
            // 稼働中: 最大 1 秒待つ。pause/skip 等の通知で早く起きる。
            let (guard, _timeout) = state
                .wake
                .wait_timeout(timer, Duration::from_secs(1))
                .unwrap();
            timer = guard;
            if timer.snapshot().status != Status::Running {
                continue; // 待機中に停止された → park へ戻る
            }
            let now = Instant::now();
            // スリープ復帰で巨大になりうるので飽和させる（u32 への wrap を防ぐ）。
            let elapsed = now.duration_since(last).as_secs().min(u32::MAX as u64) as u32;
            // 端数（1 秒未満）は次回へ持ち越し、長期ドリフトを防ぐ。
            last += Duration::from_secs(elapsed as u64);
            if elapsed == 0 {
                continue;
            }
            let events = timer.tick(elapsed);
            let snapshot = timer.snapshot();
            // ロック保持中に emit し、コマンド由来の emit と順序が入れ替わらないようにする。
            emit_events(&app, &events);
            emit_snapshot(&app, snapshot);
            // 完了数の集計（#22）。境界を跨いだ通常 tick のときだけ。スリープ復帰の巨大な
            // catch-up（elapsed が異常に大きいバッチ）は実際に作業していないので実績に数えない。
            // 保存(I/O)は timer ロックを手放してから行い、ボタン操作を待たせない。
            if !events.is_empty() && elapsed <= STATS_MAX_LIVE_GAP_SECS {
                drop(timer);
                record_stats(&app, &state, &events);
                timer = state.timer.lock().unwrap();
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state: SharedState = Arc::new(Shared {
        timer: Mutex::new(Timer::new(Config::default())),
        wake: Condvar::new(),
        // 起動時に stats.json から読み直す（apply_loaded_settings）。
        stats: Mutex::new(Stats::default()),
    });

    let mut builder = tauri::Builder::default();
    // single-instance は最初に登録する（2 回目の起動を即座に弾き、既存メインを前面化する）（#18）。
    // デスクトップ専用プラグインなので cfg(desktop) で囲う。
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }));
    }

    builder
        // 「更新を確認」でリリースページを既定ブラウザで開く。
        .plugin(tauri_plugin_opener::init())
        // 非表示中のフェーズ境界を OS トースト通知で知らせる（#20。送信はフロントから行う）。
        .plugin(tauri_plugin_notification::init())
        // OS スタートアップ登録（ログイン時に自動起動）の ON/OFF（#21。フロントから enable/disable）。
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // ウィンドウのサイズと位置を永続化・復元する（#16: ドラッグした位置を覚える）。
        .plugin(
            tauri_plugin_window_state::Builder::default()
                // サイズと位置を永続化（最大化は対象外＝復元時の位置適用との競合を避ける）。
                // 画面外に出たときの最終的な救済は自前の ensure_on_screen が担う（layout.rs / #17）。
                .with_state_flags(StateFlags::SIZE | StateFlags::POSITION)
                .build(),
        )
        .manage(state.clone())
        .setup(move |app| {
            let handle = app.handle();
            setup_tray(handle)?;
            apply_loaded_settings(handle, &state);
            spawn_tick_loop(handle.clone(), state.clone());
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // トレイ常駐: ✕ や Alt+F4 では終了せず非表示にする（終了はトレイメニューから）。
            // 設定ウィンドウも破棄せず隠す（conf で定義済みなので再表示で開き直せる）。
            WindowEvent::CloseRequested { api, .. } => {
                let label = window.label();
                if label == MAIN_LABEL || label == SETTINGS_LABEL {
                    api.prevent_close();
                    // 隠す前にサイズと位置を保存しておく（強制終了されても直近の状態が残るように）（#16）。
                    let _ = window
                        .app_handle()
                        .save_window_state(StateFlags::SIZE | StateFlags::POSITION);
                    let _ = window.hide();
                }
            }
            // 解像度/スケール/別 DPI モニタへの移動でメインが画面外に取り残されないよう
            // 作業領域へ救済する（#17）。隅へは寄せず、はみ出した分だけ最小移動する。
            // 現在のジオメトリ（outer_size/position・current_monitor）を読む best-effort 救済。
            // DPI 不変のモニタ取り外しはこのイベントが飛ばないため、その場合は次回起動時に救済する。
            WindowEvent::ScaleFactorChanged { .. } if window.label() == MAIN_LABEL => {
                if let Some(main) = window.app_handle().get_webview_window(MAIN_LABEL) {
                    let _ = layout::ensure_on_screen(&main);
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            timer_snapshot,
            timer_start,
            timer_pause,
            timer_reset,
            timer_skip,
            get_settings,
            save_settings,
            get_stats,
            reset_stats,
            open_settings,
            set_always_on_top,
            hide_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
