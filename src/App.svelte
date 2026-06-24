<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  // #1 雛形動作確認用: Rust コマンドを呼び、フロント↔バックエンドの疎通を見る。
  // 実際のタイマー UI は #3 で置き換える。
  let greeting = $state("");

  invoke<string>("greet", { name: "simpomo" })
    .then((msg) => (greeting = msg))
    .catch((err) => (greeting = `invoke error: ${err}`));
</script>

<main>
  <h1>simpomo</h1>
  <p>{greeting || "…"}</p>
</main>

<style>
  main {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    gap: 0.5rem;
  }

  h1 {
    margin: 0;
    font-size: 1.8rem;
    font-weight: 600;
  }

  p {
    margin: 0;
    font-size: 0.85rem;
    opacity: 0.7;
  }
</style>
