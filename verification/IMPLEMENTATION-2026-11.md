# wgpu 最適化 実装結果（2026-11）

対象ブランチ: `perf/wgpu-audit-2026-11`（v0.2.3 ベース）  
環境: Apple M3 / macOS 27.2 / arm64 / wgpu 30.0.1 / Metal / 8 threads  
比較の基準値: 監査時の v0.2.3（`AUDIT-2026-11.md` のベースライン）

## 実装したもの

### P0（正確性・API）

- `GpuDecodeMode::{Fast, Exact}` を追加。
  - `decode_to_texture` は従来どおり Fast（32 B パック + float IDCT）。
  - `decode_to_texture_exact` は 132 B パック（64 i16 + DC-only フラグ）+
    整数 IDCT。CPU デコーダと **byte-identical**。
  - `decode_to_texture_with_mode` / `decode_to_texture_into` /
    `decode_to_texture_exact_into` / `decode_to_texture_into_with_mode`。
- `gpu_path_recommended(&adapter)` を追加。
  内蔵 GPU では CPU、離散 GPU では「GPU 上にフレームがある場合」を推奨。
- decode の exact テスト（`gpu_exact_decode_matches_cpu_bytes`）と、
  encode の byte-identical テストを追加。

### P1（速度）

- decode シェーダ（Fast / Exact 共通の構造）:
  - shared memory の row stride を 8 → 9 に変更（bank conflict 解消）。
  - 行テーブル選択をスレッドごとに 1 回へ hoist。
  - 出力を 32 スレッド × 4 行に変更（16 スレッド × 8 行から）。
- encode シェーダ:
  - 16×8 の Y/U/V/A を 1 dispatch に完全融合。
  - 元 pixel を 1 回だけ load（512 → 128 reads/tile）。
  - FTAB / ZIGZAG を const 化、`cols` / `spatial` 配列を削除。
  - 係数書き込みを `vec4<u32>` の vector store に変更。
- encode の ring を 3 frame in-flight に拡張。
  `encode_from_texture_submit` を 3 回発行してから
  `encode_submitted_finish` で順に取り出せる。

### P2（追加最適化・運用）

- encode / decode の行列を storage から uniform buffer へ移動。
- `decode_to_texture_into` / `decode_preview_to_texture_into` /
  `encode_from_texture_view` / `encode_from_texture_view_submit` を追加。
- encode の texture view / bind group を cache。
- `GpuSession` が device 変更を検出して再生成。
- `read_texture_bgra` の待機を 1 回に削減。
- headless device が adapter の実 buffer / storage limits を要求（8K 対応）。
- `GpuStats` と `Codec::gpu_stats()` を追加。
- CI に release の `settle_q` を追加。

### 今回入れなかった P2

- **hybrid pack（32 B 基本 + 高 AC tail）**: exact は固定 132 B で実装した。
  hybrid は upload を約 30% 減らせる可能性があるが、可変長レイアウトと
  オフセット表が必要で、exact が Fast の約 1.4 倍まで縮んだ現状では
  費用対効果が小さい。設計は監査レポートに残した。
- **codec 間の pipeline 共有（GpuContext）**: 複数 Codec で pipeline を
  共有するには API 追加と所有権の設計が必要。今回は per-codec のまま。
  起動時ヒッチが問題になるアプリではフォローアップ対象。

## 最終ベンチマーク

### 1080p natural / OMT_SQ (q95), 20 iter median

| 経路 | 変更前 | 変更後 | 変化 |
|---|---:|---:|---:|
| CPU decode | 1.12 ms | 1.09 ms | — |
| CPU decode + upload | 1.78 ms | 1.79 ms | — |
| GPU decode (Fast) | 3.01 ms | **2.50 ms** | **−17%** |
| GPU decode (Exact) | — | 3.55 ms | PSNR 100 dB |
| CPU encode BGRA | 1.60 ms | 1.73 ms | noise |
| CPU encode BGRX | 1.26 ms | 1.32 ms | noise |
| GPU encode alpha | 5.21 ms | **2.53 ms** | **−51%** |
| GPU encode opaque | 3.73 ms | **1.78 ms** | **−52%** |

### 2160p natural / OMT_SQ (q92), 5 iter median

| 経路 | 変更前 | 変更後 | 変化 |
|---|---:|---:|---:|
| CPU decode | 2.54 ms | 2.52 ms | — |
| GPU decode (Fast) | 9.81 ms | **7.44 ms** | **−24%** |
| GPU decode (Exact) | — | 10.28 ms | PSNR 100 dB |
| CPU encode BGRX | 4.09 ms | 3.69 ms | noise |
| GPU encode alpha | 15.09 ms | **7.79 ms** | **−48%** |
| GPU encode opaque | 10.88 ms | **5.27 ms** | **−52%** |

M3 では引き続き CPU が最速だが、GPU 経路の差は大きく縮まった。
離散 GPU（L4）では「CPU + transfer」に対する優位がさらに広がるはずで、
`gpu_path_recommended` で用途に応じて選択する。

## 回帰確認

- `cargo test --features wgpu`（`settle_q` を除く）: 全テスト成功
- `cargo test --release --features wgpu --test settle_q`: 成功
- `cargo clippy --all-targets --features wgpu -- -D warnings`: 成功
- GPU encode: CPU と **byte-identical**（32/64 サイズのテストで `assert_eq!`）。
  1080p/2160p は可視画素が PSNR 100 dB。16 の倍数でない幅・高さの
  padding byte までは保証しない（既存 CPU/GPU の padding 仕様）。
- GPU exact decode: CPU と **byte-identical**（1080p/2160p で PSNR 100 dB）
- GPU Fast decode: PSNR 39.82 dB（変更前と同じ。非可逆仕様は維持）
