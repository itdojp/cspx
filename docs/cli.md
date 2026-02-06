# cspx CLI 仕様（v0.1）

## コマンド
- `cspx typecheck <file>`
- `cspx check --assert <ASSERT> <file>`
- `cspx check --all-assertions <file>`
- `cspx refine --model T|F|FD <spec> <impl>`

## `check --assert` のターゲット選択（v0.1）
`--assert` は **性質名** を指定する（例: `"deadlock free"`）。

`cspx check --assert "<ASSERT>" <file>` 実行時、検査対象（entry）は次の規約で決定する。

1. `<file>` 内に `<ASSERT>` に一致する property assertion（例: `assert P :[deadlock free [F]]`）が 1 件以上ある場合、**最後に出現する assertion** の `P` を entry として採用する（last-wins）。
2. 1) が無く、トップレベルの entry 式（宣言ではない process 式）がある場合、それを entry とする。
3. 1) と 2) が無く、process 宣言が 1 件のみの場合、その宣言を entry とする。
4. それ以外はエラー（entry 未指定）。

`--assert` で指定できる性質名（v0.1）:
- `"deadlock free"`
- `"divergence free"`
- `"deterministic"`

`divergence free`（v0.1）: 到達可能な τ-cycle（τ 遷移のみで閉じる cycle）が存在しないこと。

`deterministic`（v0.1）: ある状態の τ-closure 上で、同一の可視ラベルが複数の異なる τ-closure に分岐しないこと。

## `check --all-assertions`（v0.1）
`cspx check --all-assertions <file>` は `<file>` 内の `assert` 宣言を **ファイル出現順** に列挙し、`checks` 配列に格納して出力する。

- 未実装の assertion は `unsupported` + `reason.kind=not_implemented` とする。
- `checks` は複数になり得る（最低 1 件）。

## `refine`（v0.1）
`cspx refine --model T <spec> <impl>` は traces refinement（T）を検査する。

- `tau` は内部遷移として扱い、trace（可視列）からは除外する。
- `impl` が実行できる可視 trace が `spec` でも実行できること（`Traces(impl) ⊆ Traces(spec)`）。
- `fail` 時は、`impl` では実行できるが `spec` では実行できない可視 trace を counterexample として返す（最短性は保証しない）。

`--model F|FD` は未実装（現状は `unsupported` + `reason.kind=not_implemented`）。

## トップレベル status/exit_code の集約（v0.1）
`checks` が複数ある場合、トップレベルの `status`/`exit_code` は以下の優先順位で集約する。

`error` > `out_of_memory` > `timeout` > `fail` > `unsupported` > `pass`

## 共通オプション
- `--format json|text`（default: `json`）
- `--output <path>`（default: stdout）
- `--timeout-ms <n>`（任意）
- `--memory-mb <n>`（任意）
- `--seed <n>`（default: `0`）
- `--version`

## Exit code 規約
- `0`: pass
- `1`: fail
- `2`: tool error（I/O・内部例外等）
- `3`: unsupported（未対応構文・未実装機能）
- `4`: timeout
- `5`: out-of-memory

## 仕様上の注意
- `unsupported` は「機能/構文が未実装または未対応」であることを示す。
- `error` は「実行時例外・I/O 失敗・不正入力などのツールエラー」を示す。
- `--timeout-ms` / `--memory-mb` が指定されない場合、Result JSON では `null` を出力する。
- `--seed` は探索順の再現性のために使用する（v0.1 では記録のみの stub でも良い）。

## 使用例
```sh
cspx typecheck spec.cspm --format json
cspx check --assert "deadlock free" spec.cspm --format json
cspx refine --model FD spec.cspm impl.cspm --format json
```
