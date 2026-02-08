FD重経路（tau-closure 64状態リング）を再現する bench 題材。
`impl` は hiding により到達可能な tau-cycle を形成し、`spec=STOP` との FD refinement は `fail` を返す。
`counterexample.tags` に `fd_*` 計測タグ（nodes/edges/divergence_checks 等）を含める。
