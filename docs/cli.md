# cspx CLI 仕様（v0.1）

## コマンド
- `cspx typecheck <file>`
- `cspx check --assert <ASSERT> <file>`
- `cspx check --all-assertions <file>`
- `cspx refine --model T|F|FD <spec> <impl>`

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
