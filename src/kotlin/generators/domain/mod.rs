//! Domain layer generators (entities, enums, repositories)

pub use entity::{generate_entities, EntityData};
pub use enums::generate_enums;
pub use repository::generate_repositories;

mod entity;
mod enums;
mod repository;
