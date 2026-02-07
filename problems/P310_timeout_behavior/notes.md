bench 用の timeout 動作確認。
現行モデルは短時間で `pass` するため、暫定で `pass/timeout/unsupported/error` を許容する。
実運用では timeout を再現する専用モデルに置き換える。
