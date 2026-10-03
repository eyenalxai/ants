//! Layout, color and editor-cursor constants for the HUD.

use bevy::prelude::Color;

/// HUD text size in logical pixels.
pub const UI_FONT_SIZE: f32 = 16.0;
/// Padding inside HUD buttons in logical pixels.
pub const UI_BUTTON_PADDING: f32 = 8.0;
/// Inset of the HUD/FPS panels from the window edges, as a percentage.
pub const UI_EDGE_INSET_PERCENT: f32 = 1.0;
/// Padding inside the FPS panel in logical pixels.
pub const UI_FPS_PANEL_PADDING: f32 = 4.0;
/// Padding inside the top-left HUD panel in logical pixels.
pub const UI_PANEL_PADDING: f32 = 6.0;
/// Horizontal gap between HUD buttons in logical pixels.
pub const UI_HUD_GAP: f32 = 6.0;

/// Translucent background shared by all HUD panels.
pub const UI_PANEL_BACKGROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.5);
/// Idle button fill.
pub const UI_BUTTON_IDLE: Color = Color::srgba(0.3, 0.3, 0.3, 0.8);
/// Hovered button fill.
pub const UI_BUTTON_HOVERED: Color = Color::srgba(0.45, 0.45, 0.45, 0.85);
/// Pressed button fill.
pub const UI_BUTTON_PRESSED: Color = Color::srgba(0.15, 0.15, 0.15, 0.9);
/// Highlight of the Food Mode button while food mode is active.
pub const UI_BUTTON_FOOD_ACTIVE: Color = Color::srgba(0.2, 0.8, 0.2, 0.9);
/// Highlight of the Nest Mode button while nest mode is active.
pub const UI_BUTTON_NEST_ACTIVE: Color = Color::srgba(0.8, 0.2, 0.2, 0.9);
/// Highlight of the Pause button while the simulation is paused.
pub const UI_BUTTON_PAUSE_ACTIVE: Color = Color::srgba(0.8, 0.2, 0.2, 0.9);

/// Z-order of the top-left HUD panel.
pub const UI_Z_HUD: i32 = 10;
/// Z-order of the paused indicator, above the HUD.
pub const UI_Z_PAUSED: i32 = 20;
/// Z-order of the FPS panel.
pub const UI_Z_FPS: i32 = 30;

/// Top offset of the paused indicator as a percentage, just below the HUD.
pub const UI_PAUSED_TOP_PERCENT: f32 = 9.0;
/// Text shown by the paused indicator.
pub const UI_PAUSED_TEXT: &str = "PAUSED";
/// Font size of the paused indicator in logical pixels.
pub const UI_PAUSED_FONT_SIZE: f32 = 28.0;
/// Color of the paused indicator text.
pub const UI_PAUSED_TEXT_COLOR: Color = Color::srgba(1.0, 0.3, 0.3, 1.0);

/// FPS value at or above which the counter is green.
pub const UI_FPS_GOOD: f64 = 120.0;
/// FPS value at or above which the counter blends from green to yellow.
pub const UI_FPS_OK: f64 = 60.0;
/// FPS value at or above which the counter blends from yellow to red.
pub const UI_FPS_LOW: f64 = 30.0;

/// Side length of the square food brush in cells (3 means a 3×3 brush).
pub const FOOD_BRUSH_DIAMETER: i32 = 3;
/// Fill color of the food brush cursor.
pub const FOOD_CURSOR_COLOR: Color = Color::srgba(0.2, 0.8, 0.2, 0.5);
/// Side length of the square nest cursor in world units.
pub const NEST_CURSOR_SIZE: f32 = 20.0;
/// Nest cursor fill while no drag is in progress.
pub const NEST_CURSOR_IDLE_COLOR: Color = Color::srgba(1.0, 0.0, 0.0, 0.4);
/// Nest cursor fill while the nest is being dragged.
pub const NEST_CURSOR_DRAG_COLOR: Color = Color::srgba(1.0, 0.0, 0.0, 0.7);
