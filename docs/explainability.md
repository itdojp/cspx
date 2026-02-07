# Explainability ガイド（v0.1）

## 目的
`cspx` の Explainability は「検査できる」だけでなく「修正に繋げられる」ことを目的にする。  
そのため、反例 JSON は次の3点を最低保証とする。

- 原因分類（`tags`）
- 位置特定（`source_spans`）
- 過剰に長くない反例（`events` + `is_minimized`）

## 反例品質の要件
### 1) tags（原因分類）
反例 `tags` は次の体系で付与する。

- 主要カテゴリ: `deadlock` / `divergence` / `nondeterminism` / `refinement`
- モデル識別: `model:T` / `model:F` / `model:FD`
- 詳細原因: `trace_mismatch` / `refusal_mismatch` / `divergence_mismatch` / `label:<event>` / `refuse:<event>`
- Explainer 付与: `kind:<カテゴリ>` / `explained`

運用方針:
- 推測値を直接出さず、検査結果から導出可能な情報だけをタグ化する。
- 将来タグを追加する場合は、既存タグの意味を変更しない（後方互換）。

### 2) source_spans（位置情報）
`source_spans` は少なくとも次を持つ。

- `path`
- `start_line` / `start_col`
- `end_line` / `end_col`

運用方針:
- assertion ターゲットが特定できる場合は、その process span を優先する。
- 精度が不明なときは、誤った span を出すより欠損（空配列）を選ぶ。

### 3) 反例長と最小化
- `counterexample.events` は診断に必要な最短側を目指す。
- `is_minimized=true` は oracle 検証済み最小化のみを意味する。
- `is_minimized=false` は未最小化、または最小性未保証を意味する。

## JSON 安定性（diff 安定化）
同一入力で不要な差分が出ないことを重視する。

- `checks` の順序は入力（assert 宣言順）に準拠する。
- `tags` は重複除去し、意味のある最小集合に保つ。
- `problems` の比較では `normalized_json_equal` を利用し、時刻等の揺らぐフィールドを除外する。

## Problem Suite での回帰化
Explainability の品質は `problems` の制約評価で固定化する。

- `P300`: 反例の短さ（trace 長）を検証
- `P301`: `source_spans` の位置妥当性を検証
- `P302`: 同一入力の JSON 決定性を検証

`expect.yaml` では完全一致ではなく制約（`contains` / `min` / `max`）を使い、将来拡張を阻害しない。

## 既知の制約（v0.1）
- 反例最小化はケースにより未適用のため、常に `is_minimized=true` にはならない。
- 高コストな最小化は fast suite では抑制し、必要時に個別検証する。

## 関連
- `docs/result-json.md`
- `docs/cli.md`
- `problems/README.md`
