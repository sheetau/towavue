//! Shared UI values. Media pixels and authored subtitle colors are not UI tokens.
use egui::Color32;

pub const BACKGROUND: Color32 = Color32::BLACK;
pub const FLOATING_BACKGROUND: Color32 = Color32::from_gray(0x09);
pub const SURFACE: Color32 = Color32::from_gray(0x12);
pub const HOVER: Color32 = Color32::from_gray(0x16);
pub const KEY_ACTIVE: Color32 = Color32::from_gray(0x1f);
pub const DISABLED: Color32 = Color32::from_gray(0x2a);
pub const FAINT: Color32 = Color32::from_gray(0x42);
pub const MUTED: Color32 = Color32::from_gray(0x75);
pub const FOREGROUND: Color32 = Color32::from_gray(0xe0);
pub const ACTIVE_TAB: Color32 = SURFACE;
pub const BORDER: Color32 = SURFACE;
pub const TRACK: Color32 = DISABLED;
pub const SURFACE_HOVER: Color32 = SURFACE;

pub const RADIUS_XS: u8 = 3;
pub const RADIUS_SM: u8 = 6;
pub const TEXT_SM: f32 = 9.0;
pub const TEXT_MD: f32 = 11.0;
pub const TEXT_LG: f32 = 12.0;
pub const ITEM_GAP: f32 = 2.0;
pub const MENU_PADDING: i8 = 3;
pub const PADDING: i8 = 5;
pub const CONTROL_HEIGHT: f32 = 22.0;
pub const ROW_HEIGHT: f32 = 24.0;
pub const ICON_SIZE: f32 = 16.0;
