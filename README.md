# Ants

Real-time 2D ant colony simulation built with [Bevy](https://bevyengine.org/) 0.19.1.
Ants forage using two pheromone trails (to food and back to the nest), the colony
grows to up to 30,000 ants, and both food sources and the nest can be edited at
runtime. Debug overlays render the pheromone grid and the sensor cone of one ant.

## Prerequisites

The project requires **Rust 1.95 or newer** (Bevy 0.19.1's MSRV; CI checks the
exact 1.95.0 toolchain). On Linux, Bevy also needs a few system development
packages. On Debian/Ubuntu:

```sh
sudo apt-get install -y \
  libasound2-dev \
  libudev-dev \
  libwayland-dev \
  libxkbcommon-dev \
  libx11-dev
```

CI installs exactly this list. On other distributions, install the equivalent
ALSA, udev, Wayland, xkbcommon and X11 development packages; see Bevy's
[platform setup guide](https://bevy.org/learn/quick-start/getting-started/setup/)
for per-platform instructions.

## Quick start

```sh
cargo run
```

For faster iterative builds, opt into Bevy's dynamic linking (see
[Dynamic linking](#dynamic-linking)):

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
| `core` | Grid geometry, render layers and schedule sets. |
| `constants` | Tunable simulation constants (world, ants, sensors, pheromones). |
| `simulation` | Ants, food, movement and pheromone deposit systems. |
| `pheromone` | Two-pheromone grid storage, decay and queries. |
| `overlays` | Debug rendering: pheromone visualization and the sensor cone. |
| `editor` | Runtime editing of food and the nest. |
| `ui` | HUD: buttons, pause control and the FPS counter. |

Simulation runs in `FixedUpdate` at a fixed 64 Hz, so ant behaviour is
decoupled from the render frame rate. Pausing freezes virtual time, which stops
the fixed-step simulation while overlays, UI and editing keep responding.

### Randomness

Every random draw (ant spawn jitter, steering noise, overlay ant selection)
currently comes from the process-global `fastrand` generator. There is no seed
and no simulation-owned RNG resource yet, so two runs with the same inputs are
**not** reproducible; treat each run as an independent sample.

### Testing

The unit tests are headless: they build Bevy `App`s without a window, GPU or
audio device, so `cargo test` works in CI and over SSH.

```sh
cargo test --locked
```

### Dynamic linking

`dynamic_linking` is an opt-in Cargo feature wrapping Bevy's
`bevy/dynamic_linking`. It links Bevy as a shared library, which speeds up
incremental dev builds at the cost of slower startup and a non-portable binary.
Use it for local iteration only.

### Logging

Bevy logs through `tracing`. The direct `log` dependency does not appear in the
source: it exists only to cap the global `log` records emitted by third-party
crates (debug and below in dev builds, warn and below in release), as explained
in `Cargo.toml`.

## Development

```sh
cargo fmt --all
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --locked
```

There is no CI; run these checks locally before committing.

### Local linker configuration

`.cargo/config.toml` is machine-specific and untracked, so other machines
build with the standard toolchain and linker. To opt into faster dev builds on
this machine:

```sh
cp .cargo/config.toml.example .cargo/config.toml
```

The example uses the nightly-only cranelift codegen backend for the local crate
(dependencies stay on LLVM) and the `mold` linker via `clang`; it requires a
nightly toolchain plus `clang` and `/usr/bin/mold`. Without it, builds work on
stable with no extra setup.
