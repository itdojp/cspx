# ae-framework 統合契約（v0.1）

## 目的
`ae-framework` の `verify-csp` から `cspx` を呼び出し、CI で再現可能な成果物を生成するための契約を定義する。  
この文書は `Issue #106`（案A）の実装基準とする。

## 契約（Contract）
### 入力契約
- 実行コマンド:
  - `cspx typecheck <file>`
  - `cspx check --assert "deadlock free" <file>`
  - `cspx refine --model T|F|FD <spec> <impl>`
- 共通オプション:
  - `--format json`
  - `--output <result-json-path>`
  - `--summary-json <summary-json-path>`

### 出力契約
- Result JSON:
  - 仕様: `schemas/cspx-result.schema.json`
  - `schema_version` は `0.1` 固定
  - `metrics` は互換拡張（optional）。consumer は未知フィールドを無視してよい
- 集約サマリ:
  - 仕様: `schemas/csp-summary.schema.json`
  - `tool` は `csp` 固定
  - `ran` は `true` 固定（`cspx` 自体が summary を生成できたことを意味する）
  - `backend` は `cspx:typecheck|assertions|refine`
  - `status` は `ran|failed|unsupported|timeout|out_of_memory|error`
  - `resultStatus` は cspx の `status` を保持

### exit code 契約
- `0`: pass
- `1`: fail
- `2`: error
- `3`: unsupported
- `4`: timeout
- `5`: out_of_memory

## 推奨呼び出し（ae-framework 側）
### typecheck
```sh
cspx typecheck spec/csp/cspx-smoke.cspm \
  --format json \
  --output artifacts/hermetic-reports/formal/cspx-result.json \
  --summary-json artifacts/hermetic-reports/formal/csp-summary.json
```

### assertions（v0.1 最小）
```sh
cspx check --assert "deadlock free" spec/csp/sample.cspm \
  --format json \
  --output artifacts/hermetic-reports/formal/cspx-result.json \
  --summary-json artifacts/hermetic-reports/formal/csp-summary.json
```

## CI運用前提
- `cspx` バージョンはタグ/コミット固定で導入する（再現性確保）。
- `verify-csp` は non-blocking 運用でも、`csp-summary.json` を常に生成する。
- `schema_version != 0.1` の場合は互換外として `unsupported` 扱いとする。
- `schema_version == 0.1` では `schemas/cspx-result.schema.json` に定義されたフィールドを互換範囲とする（`metrics` はこのスキーマで定義済みの optional 拡張）。
- consumer が strict schema validation を行う場合、未知フィールドは受理されない前提で運用する。

## セキュリティ前提
- 信頼できない入力（fork PR 等）で任意コマンド経路を実行しない。
- `CSP_RUN_CMD` を使う場合は trusted-only で運用する。
- 成果物には要約と詳細を分離して保存し、PR コメントへの過剰出力を避ける。

## 既知制約（v0.1）
- assertions モードは `deadlock free` を最小導線として想定する。
- 高コストな最小化や大規模ベンチは別フェーズ（案C）で扱う。

## 関連
- `docs/cli.md`
- `docs/result-json.md`
- `schemas/csp-summary.schema.json`
- `docs/validation-report.md`
