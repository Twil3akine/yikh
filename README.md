<img src="src-tauri/icons/icon.png" alt="Yikhアイコン" width="112" />

# Yikh

TaskとButeをローカルのSQLiteに保存するmacOS向けタスク管理アプリです。GUIで追加・編集・完了・削除でき、llama.cpp上のOrnithには検索・相談やアイテム操作を依頼できます。

## 起動

Bun、Rust、Xcode Command Line Toolsを用意して、プロジェクト内で実行します。以下のコマンドはApple SiliconのMac向けです。

```sh
bun install
CC=/usr/bin/clang CXX=/usr/bin/clang++ \
CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/usr/bin/clang \
SDKROOT="$(xcrun --show-sdk-path)" bun run tauri dev
```

## Assistant

Assistantを使う場合は、llama.cppの`llama-server`を用意し、別のターミナルで起動します。モデルはOrnith 1.5 9BのQ4_K_Mです。初回はモデルがダウンロードされます。

```sh
llama-server -hf ornith-ai/Ornith-1.5-9B-GGUF:Q4_K_M --port 8000 --jinja
```

接続先の既定値は`http://127.0.0.1:8000/v1`です。llama-serverが停止していても、GUIでのタスク管理は利用できます。

Assistantからの追加・編集・完了・削除は、内容を確認してから実行します。対象が曖昧な場合は、候補を選んだ後に確認します。「1ヶ月後」「1年後」は現在日を基準に暦で計算します。

会話はSQLiteに保存され、右サイドバーの「履歴」から再開できます。上部の＋で新しい会話を作成し、…メニューから現在の会話を削除できます。

## 操作

左にアイテム一覧、中央に詳細、右にAssistantを表示します。左右のサイドバーは境界をドラッグして幅を変更できます。

| ショートカット | 操作 |
| --- | --- |
| ⌘N | アイテムを新規追加 |
| ⌘B | 左サイドバーを開閉 |
| ⌘K | 右サイドバーを開閉。開くと入力欄へフォーカス |
| Enter | Assistantへ送信。日本語の変換確定中は送信しません |
| Shift + Enter | Assistant入力欄で改行 |

All / Tasks / Butesで一覧を絞り込みます。数字は未完了の件数です。締切が早い順に並び、同じ締切なら更新が新しい順、締切なしは末尾に並びます。完了済みは「完了済みも表示」で確認できます。

アイテムと会話の保存先は、macOSでは`~/Library/Application Support/dev.rutt.yikh/yikh.sqlite3`です。

## ビルド

```sh
CC=/usr/bin/clang CXX=/usr/bin/clang++ \
CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/usr/bin/clang \
SDKROOT="$(xcrun --show-sdk-path)" bun run tauri build
```

アプリは`src-tauri/target/release/bundle/macos/Yikh.app`に生成されます。

## ライセンス

[MIT License](LICENSE)
