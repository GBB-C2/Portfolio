# Raster shape preview benchmark

These are native Linux measurements of the same dependency-free Rust raster code used by Portfolio. They do not measure Windows message handling, GDI, the complete editor frame, or Windows FPS.

## Conditions

- Intel Xeon Platinum 8573C, shared Linux x86-64 host; wall-clock timings can vary with scheduling and CPU frequency.
- Rust optimized build: `-O -C lto=yes -C codegen-units=1`.
- A white paper layer and a transparent paint layer; ellipse endpoints at 15% and 85% of document dimensions.
- Opacity 65%, flow 100%, hardness 100%, source RGB (230,40,30).
- Ordinary cases: Ink, spacing 0.12, mirror disabled.
- Dense cases: Ink or Soft, size 512, spacing 0.02, mirror enabled.
- One initial preview followed by nine changed geometries, incrementing the endpoint by (0.4,0.3). `update_median_ms` includes restoration of the previous preview and rasterization of the new one. Composite recomposition is measured separately.
- Repeated identical geometry is measured over 10,000 calls. Commit is measured separately and does not add stamps.

## Recorded median update time

| Case | Document | Size | Before, ms | After, ms | Reduction |
|---|---|---:|---:|---:|---:|
| ink | 1280×800 | 96 | 23.962 | 9.619 | 59.9% |
| ink | 1280×800 | 512 | 51.790 | 40.477 | 21.8% |
| ink | 1920×1080 | 96 | 15.407 | 13.405 | 13.0% |
| ink | 1920×1080 | 512 | 71.389 | 66.892 | 6.3% |
| dense_mirror_ink | 1920×1080 | 512 | 765.263 | 401.496 | 47.5% |
| dense_mirror_soft | 1920×1080 | 512 | 833.732 | 721.126 | 13.5% |

The dense Ink case improves substantially. The dense Soft case remains expensive: about 0.72 seconds per changed preview in this run. Coalescing geometry changes to one render avoids redundant queued work, but does not make that individual computation inexpensive. These measurements do not establish a universal responsiveness or frame-rate guarantee.

The final implementation caches the exact alpha table and nonzero row bounds, and skips mathematically unchanged saturated paint pixels and completely transparent eraser pixels. A trial cache of the most recently captured tile span was removed because its speed improvement was not established. Stamp count, spacing, order, geometry and integer blending results are unchanged.

## Pixel equivalence gate

`stamp_equivalence_gate.rs` compiles both the immutable baseline source and the current document source. It compares every pixel byte and history/dirty state at 3,960 checkpoints across 480 configurations: all ten brush kinds × paint/erase × mirror on/off × three opacity values × four sizes (3,37,96,512). Scenarios cover clipped strokes, compensation followed by ordinary segments and the endpoint, live line/rectangle/ellipse replacement, commit, undo, redo and cancel. Imported transparent pixels retain hidden RGB in the baseline; zero-source-alpha erasing is included to verify its normalization behavior.

The CSV files contain compact FNV-1a checksums for those checkpoints, while the executable gate compares the complete bytes in memory. Both CSV files have SHA-256:

`a4f25b7c1a45906438820e08503f80999e247c541591b63ef43246e8453264a7`

All 33 document tests passed, including a scalar full-mask oracle for cached alpha/reflected bounds and the live-shape transaction regressions.

## Reproduce from the repository root

Use a Rust toolchain and an existing writable `target` directory. Run the timing commands sequentially without other CPU-heavy tasks.

```sh
rustc --edition=2021 -O -C lto=yes -C codegen-units=1 --cfg baseline docs/benchmarks/shape_preview_timing.rs -o target/shape-preview-before
rustc --edition=2021 -O -C lto=yes -C codegen-units=1 docs/benchmarks/shape_preview_timing.rs -o target/shape-preview-after
target/shape-preview-before > target/shape-timing-before.csv
target/shape-preview-after > target/shape-timing-after.csv
rustc --edition=2021 -O -C lto=yes -C codegen-units=1 docs/benchmarks/stamp_equivalence_gate.rs -o target/stamp-equivalence-gate
target/stamp-equivalence-gate target/stamp-pixels-after.csv compare
```

The immutable baseline is `document_before_stamp_optimization.rs`. It is benchmark evidence, not a compiled application module. The application uses `src/document.rs`.
