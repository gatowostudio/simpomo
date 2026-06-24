# クレジット / 出典

## 通知音

simpomo の通知音は**音源ファイルを同梱していません**。すべて実行時に
[Web Audio API](https://developer.mozilla.org/docs/Web/API/Web_Audio_API) で合成した
短い音（正弦波 + エンベロープ）です。実装は `src/lib/sounds.ts`。

- ライセンス: 自作（合成）のため第三者の著作物を含みません。配布・改変に制約はありません。
- 同梱バイナリ音源（mp3/wav 等）はありません（軽量方針・公開リポでの著作権配慮、CLAUDE.md「Don't」）。

将来、合成ではなく音源ファイルを採用する場合は、**ロイヤリティフリーまたは自作のもののみ**を
同梱し、その出典・ライセンスをこのファイルに必ず記録すること。

## アイコン

アプリアイコンは**自作**のトマト（ポモドーロ）モチーフ。ソースは `src-tauri/icons/app-icon.svg`。
`src-tauri/icons/` 配下の各 PNG / `.ico` / `.icns` はこの SVG から `tauri icon` で生成している
（再生成手順は `docs/development.md`）。第三者の著作物は含まない。
