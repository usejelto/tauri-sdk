//! Jelto's Tauri 2 desktop SDK. Registration is inactive until explicit initialization.
//! See the README for the public API and the published setup guide at
//! <https://jelto.io/docs/sdk/tauri.md>; the wire and behavioural contracts the plugin
//! implements live in the public contracts repository (`spec/wire-v1.md`,
//! `spec/sdk-conformance.md` at <https://github.com/usejelto/contracts>).
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
