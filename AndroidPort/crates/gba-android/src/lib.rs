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
mod pub_touch;
mod deploy_touch;
mod battle_ui;
mod battle_windows;
mod text_speed;
mod title_touch;
mod world_map;
pub mod game_buttons;
pub mod menu_layout;
mod scheduling;
