# simpomo

シンプルで軽量なポモドーロタイマーのデスクトップアプリ（Windows / macOS / Linux）。

画面の隅に小さく常駐し、基本は常に最前面に表示。ボタン一つで最前面モードを切り替えられます。
作業/休憩時間・サイクル数・通知音・ウィンドウサイズをカスタマイズ可能。

- 仕様: [`docs/spec.md`](docs/spec.md)
- 技術選定の経緯: [`docs/decisions/0001-initial-stack.md`](docs/decisions/0001-initial-stack.md)
- 開発・ビルド手順: [`docs/development.md`](docs/development.md)

## インストール

各 OS 向けインストーラは [GitHub Releases](https://github.com/gatowostudio/simpomo/releases)
で配布します（タグ push で各 OS 版をビルドする CI を用意済み）。ソースからのビルドは
[`docs/development.md`](docs/development.md) を参照してください。

## Stack

Tauri v2（Rust）+ Svelte（Vite / TypeScript）

> 開発中です。
