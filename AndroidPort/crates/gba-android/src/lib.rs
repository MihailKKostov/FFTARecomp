//! Android frontend for the desktop hardware model and C recompiler.
mod compiler;
#[doc(hidden)]
pub mod engine;
mod jni;

mod audio;
mod normal_audio;
mod ui_layout;
mod touch;
mod scheduling;
