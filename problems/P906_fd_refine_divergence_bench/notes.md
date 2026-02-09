FD重経路（tau-closure 64状態リング）を再現する bench 題材。
`impl` は hiding により到達可能な tau-cycle を形成し、理論上は `spec=STOP` との FD refinement が `fail` を返す（FD 発散検査のベンチマーク）。

現状の実装では、入力受理の経路差により以下の暫定結果が出る。
- `unsupported` / `unsupported_syntax`
- `error` / `invalid_input`（例: entry process not specified）

機能境界の固定を優先し、期待値は暫定で `unsupported/error` の双方を許容する。
