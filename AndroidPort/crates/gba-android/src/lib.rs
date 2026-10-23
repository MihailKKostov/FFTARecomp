//! Android frontend for the desktop hardware model and C recompiler.
mod compiler;
#[doc(hidden)]
pub mod engine;
mod jni;

mod audio;
mod normal_audio;
mod ui_layout;
pub mod touch;
mod gestures;
mod shop_scroll;
mod laws;
mod title_touch;
mod world_map;
pub mod game_buttons;
pub mod menu_layout;
mod scheduling;
