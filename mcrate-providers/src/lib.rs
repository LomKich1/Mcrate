//! mcrate-providers: реализации MusicProvider для конкретных сервисов.

pub mod local;
pub use local::LocalProvider; // чтобы писать mcrate_providers::LocalProvider