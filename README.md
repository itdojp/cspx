# cspx

cspx は、CSPM の検査（typecheck/assertion/refinement）を CI で再現性高く実行するための、CI-first / explainability / extensibility 指向のモデル検査ツールです。

## 目的
- CI で再現可能な検査実行（機械可読な JSON 出力、安定した exit code）
- 拡張可能なアーキテクチャ（プラグイン境界・中間表現の明確化）
- 反例の説明性（短い反例、原因タグ、ソース位置）

## ドキュメント
- CLI 仕様: `docs/cli.md`
- Result JSON 仕様: `docs/result-json.md`
- アーキテクチャ: `docs/architecture.md`
- 拡張システム全体像: `docs/system-overview.md`
- Explainability ガイド: `docs/explainability.md`
- ae-framework 統合契約: `docs/integrations/ae-framework.md`
- 有用性・動作検証レポート: `docs/validation-report.md`
- Problem Suite（回帰/デモ用 問題集）: `problems/README.md`

## 開発手順（最小）
```sh
cargo build
cargo test
```

## Docker（任意）
```sh
docker build -t cspx:local .
docker run --rm cspx:local --version
```

## ライセンス
Apache-2.0
