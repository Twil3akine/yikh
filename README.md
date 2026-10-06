<img src="src-tauri/icons/icon.png" alt="Yikhアイコン" width="112" />

# Yikh

TaskとButeをローカルのSQLiteに保存するmacOS向けタスク管理アプリです。llama.cpp上のOrnithに、アイテムの検索・整理や次に進めることを相談できます。

## 起動

Bun、Rust、Xcode Command Line Toolsを用意して、プロジェクト内で実行します。

```sh
bun install
CC=/usr/bin/clang CXX=/usr/bin/clang++ \
CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/usr/bin/clang \
SDKROOT="$(xcrun --show-sdk-path)" bun run tauri dev
```

Assistantを使う場合は、別のターミナルでllama-serverを起動します。

```sh
llama-server -hf ornith-ai/Ornith-1.5-9B-GGUF:Q4_K_M --port 8000 --jinja
```
