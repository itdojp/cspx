# Result JSON 仕様（v0.1）

`--format json` の標準出力（または `--output`）は、常に本仕様に従う。

## トップレベル
| フィールド | 型 | 必須 | 説明 |
|---|---|---|---|
| `schema_version` | string | yes | 固定値 `"0.1"` |
| `tool` | object | yes | ツール情報（`name`, `version`, `git_sha`） |
| `invocation` | object | yes | 実行情報（`command`, `args`, `format`, `timeout_ms`, `memory_mb`, `seed`） |
| `inputs` | array | yes | 入力一覧（`path`, `sha256`） |
| `status` | enum | yes | `pass | fail | unsupported | timeout | out_of_memory | error` |
| `exit_code` | integer | yes | CLI の exit code と一致 |
| `started_at` | string | yes | RFC3339 / UTC（例: `2026-02-04T00:00:00Z`） |
| `finished_at` | string | yes | RFC3339 / UTC |
| `duration_ms` | integer | yes | 実行時間（ミリ秒） |
| `checks` | array | yes | チェック結果（少なくとも1件） |

## `checks` 要素
| フィールド | 型 | 必須 | 説明 |
|---|---|---|---|
| `name` | string | yes | `typecheck` / `check` / `refine` |
| `model` | string or null | yes | `T` / `F` / `FD`（typecheck/check は null） |
| `target` | string or null | yes | assertion 名、または refine の対象記述 |
| `status` | enum | yes | トップレベルと同義 |
| `reason` | object | no | `status` が `pass` 以外の理由 |
| `counterexample` | object or null | no | v0.1 では null でも可 |
| `stats` | object | no | `states` / `transitions`（null 可） |

### `reason.kind`（enum）
- `not_implemented`
- `unsupported_syntax`
- `invalid_input`
- `internal_error`
- `timeout`
- `out_of_memory`

## Counterexample（v0.1 形状）
```json
{
  "type": "trace",
  "events": [{"label": "a.1"}, {"label": "b"}],
  "is_minimized": false,
  "tags": ["deadlock"],
  "source_spans": [
    { "path": "spec.cspm", "start_line": 12, "start_col": 3, "end_line": 12, "end_col": 25 }
  ]
}
```

### `counterexample.is_minimized` の意味（v0.1）
- `true`: minimizer が oracle で fail 維持を検証し、局所最小（1イベント削除で fail を維持できない）を確認済み。
- `false`: 最小化未実施、または fail 維持を検証できず最小性を主張できない状態。

## 例（トップレベル）
```json
{
  "schema_version": "0.1",
  "tool": { "name": "cspx", "version": "0.1.0", "git_sha": "UNKNOWN" },
  "invocation": {
    "command": "typecheck",
    "args": ["spec.cspm"],
    "format": "json",
    "timeout_ms": null,
    "memory_mb": null,
    "seed": 0
  },
  "inputs": [
    { "path": "spec.cspm", "sha256": "..." }
  ],
  "status": "unsupported",
  "exit_code": 3,
  "started_at": "2026-02-04T00:00:00Z",
  "finished_at": "2026-02-04T00:00:00Z",
  "duration_ms": 12,
  "checks": [
    {
      "name": "typecheck",
      "model": null,
      "target": null,
      "status": "unsupported",
      "reason": { "kind": "not_implemented", "message": "typechecker not implemented yet" },
      "counterexample": null,
      "stats": { "states": null, "transitions": null }
    }
  ]
}
```
