# Ants

Real-time 2D ant colony simulation built with [Bevy](https://bevyengine.org/) 0.17.
Ants forage using two pheromone trails (to food and back to the nest), the colony
grows to up to 30,000 ants, and both food sources and the nest can be edited at
runtime. Debug overlays render the pheromone grid and the sensor cone of one ant.

## Quick start

```sh
cargo run
```

For faster iterative builds, opt into Bevy's dynamic linking:

```sh
cargo run --features dynamic_linking
```

## Controls

| Control | Action |
| --- | --- |
| **Pause** button | Freeze/resume the simulation; the button turns red while paused. |
| **Food Mode** button | Toggle food editing. Left-drag paints a 3×3 food brush, right-drag erases. |
| **Nest Mode** button | Toggle nest editing, then left-drag the nest to move it. |
| **F3** | Toggle the pheromone overlay and pick a random ant for the sensor-cone debug view. |
| **F12** | Toggle the FPS counter. |

## Architecture

The code is organized into these modules:

| Module | Responsibility |
| --- | --- |
| `core` | Grid geometry and world ↔ grid coordinate conversions. |
| `constants` | Tunable simulation constants (world, ants, sensors, pheromones). |
| `simulation` | Ants, food, movement and pheromone deposit systems. |
| `pheromone` | Two-pheromone grid storage, decay and queries. |
| `overlays` | Debug rendering (pheromone visualization, sensor cone, FPS counter). |
| `editor` | Runtime editing of food and the nest. |
| `ui` | Buttons and other interface elements. |

Simulation runs in `FixedUpdate`, so ant behaviour is decoupled from the render
frame rate. Pausing freezes virtual time, which stops the fixed-step simulation
while overlays, UI and editing keep responding.

## Development

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
cargo build
```

CI runs the same checks on stable Rust (see `.github/workflows/ci.yml`).

### Local linker configuration

`.cargo/config.toml` is machine-specific and untracked, so CI and other machines
build with the standard toolchain and linker. To opt into faster dev builds on
this machine:

```sh
cp .cargo/config.toml.example .cargo/config.toml
```

The example uses the nightly-only cranelift codegen backend for the local crate
(dependencies stay on LLVM) and the `mold` linker via `clang`; it requires a
nightly toolchain plus `clang` and `/usr/bin/mold`. Without it, builds work on
stable with no extra setup.
