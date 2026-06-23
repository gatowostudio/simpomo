# 開発手順 — simpomo

Tauri v2（Rust）+ Svelte（Vite / TypeScript）構成のローカル開発メモ。
※ プロジェクトの雛形（`pnpm create tauri-app` 等）はまだ未作成。下記コマンドは構成確定後の想定で、
  実際のスクリプト名は雛形作成後に確定したら更新する。

## 前提ツール

- **Rust** ツールチェイン（`rustup`、安定版）+ `cargo`
- **Node.js**（LTS）+ **pnpm**
- 各 OS の Tauri 前提パッケージ:
  - Windows: Microsoft C++ Build Tools / WebView2（多くの環境は既定で導入済み）
  - macOS: Xcode Command Line Tools
  - Linux: `webkit2gtk` 等（ディストリ依存）
  - 参照: https://v2.tauri.app/start/prerequisites/

## セットアップ

```sh
pnpm install          # フロントエンド依存の取得
```

## 開発（ホットリロード）

```sh
pnpm tauri dev        # Vite + Tauri を起動。UI 変更は即反映
```

## ビルド（配布物の生成）

```sh
pnpm tauri build      # 各 OS 向けインストーラ/実行ファイルを生成
```

生成物は `src-tauri/target/release/bundle/` 配下に出る（OS により .msi/.exe, .dmg, .AppImage/.deb）。

## テスト

```sh
cd src-tauri
cargo test            # コアロジック（タイマー進行・サイクル遷移）の単体テスト
```

テスト方針は CLAUDE.md「テスト方針」を正とする（コアロジックのみ単体テスト、UI は手動確認）。

## ディレクトリ構成（想定）

```
simpomo/
├─ src/              # Svelte フロントエンド（UI）
├─ src-tauri/        # Rust バックエンド（コアロジック・ウィンドウ制御）
│  └─ src/           # タイマー進行・サイクル状態など
├─ docs/             # 仕様・設計判断
└─ ...
```

## 公開リポジトリでの注意

- 署名鍵 / updater 秘密鍵 / `.env` は **絶対にコミットしない**（`.gitignore` 済み）。
- 同梱する通知音はロイヤリティフリー / 自作のもののみ。出典・ライセンスを記録する。
