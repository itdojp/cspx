FD重経路（tau-closure 64状態リング）を再現する bench 題材。
`impl` は hiding により到達可能な tau-cycle を形成し、理論上は `spec=STOP` との FD refinement が `fail` を返す（FD 発散検査のベンチマーク）。ただし現状の problems runner では frontend 制約により `unsupported` / `unsupported_syntax` 扱いとなる。
`counterexample.tags` に `fd_*` 計測タグ（nodes/edges/divergence_checks 等）を含める。
