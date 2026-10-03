# AGENTS.md

Conventions for automated contributors working on this repository.

## No CI

This project intentionally has **no CI/CD**. Do not add GitHub Actions
workflows, Dependabot, cargo-deny/cargo-audit automation, or any other CI
configuration. Run the checks locally before committing:

```sh
cargo fmt --all
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --locked
```

## Structure

- `src/core/` — grid, layers, system sets (shared primitives)
- `src/constants/` — all tuning constants, grouped by feature
- `src/simulation/` — ants (with per-ant deterministic RNG), movement, food, density, colony, deposits
- `src/pheromone/` — pheromone grid and decay
- `src/overlays/` — pheromone and sensor-cone debug rendering
- `src/editor/` — food/nest tools and cursors
- `src/ui/` — HUD and FPS overlay
- `src/perf.rs` — opt-in performance counters (`ANTS_PERF=1`)

Feature plugins are wired in `src/main.rs`; the simulation runs in
`FixedUpdate` at 64 Hz.

## Tests

`cargo test` is headless (no GPU/window required). Behavioral tests should be
deterministic and assert observable behavior rather than internal bookkeeping.
