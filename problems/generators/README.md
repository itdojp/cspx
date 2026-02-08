# Bench Problem Generators

## 目的
- `P900`〜`P905` の `model.cspm` を再生成可能にし、bench 問題の再現性を維持する。
- tiny/medium の規模差を固定し、将来の性能比較の基準を揃える。

## 再生成コマンド
```sh
problems/generators/regenerate_p900_p905.sh
```

上記コマンドは以下を更新する。
- `P900_ring_n_generator/model.cspm`（ring tiny）
- `P901_dining_philosophers_small/model.cspm`（philosopher loops tiny）
- `P902_abp_tiny/model.cspm`（ABP tiny）
- `P903_ring_medium/model.cspm`（ring medium）
- `P904_dining_philosophers_medium/model.cspm`（philosopher loops medium）
- `P905_abp_medium/model.cspm`（ABP medium）

## 固定パラメータ（現行）
- ring: tiny=`N=4`, medium=`N=16`
- philosopher loops（interleaving 近似）: tiny=`K=3`, medium=`K=5`
- ABP: tiny=`0..1`, medium=`0..3`

## 検証手順
```sh
cargo build -p cspx --release
scripts/run-problems --suite bench --cspx target/release/cspx --only P900 --only P901 --only P902 --only P903 --only P904 --only P905
```

`expect.yaml` は完全一致ではなく下限制約（`stats.min`）で評価するため、過剰拘束を避けつつ規模差の退行を検知できる。
