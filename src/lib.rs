//! Jelto's Tauri 2 desktop SDK. Registration is inactive until explicit initialization.
//! See the repository's `spec/tauri-sdk.md` and `docs/sdk/tauri.md`.
mod engine;
mod store;
mod wire;

pub use engine::Jelto;
pub use wire::{PropValue, Props};

#[cfg(feature = "tauri")]
mod adapter;
#[cfg(feature = "tauri")]
pub use adapter::{init, JeltoExt};

#[cfg(test)]
mod tests;
