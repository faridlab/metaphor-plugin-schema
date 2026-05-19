//! Merge-aware file writers.
//!
//! Generators emit fresh content on every run, but the schema command never
//! overwrites user customizations blindly. Files are categorised and routed
//! to the strategy that fits their shape:
//!
//! | File pattern                        | Strategy                       |
//! |-------------------------------------|--------------------------------|
//! | `config/application*.yml`           | [`yaml_config`] — user values win |
//! | `seed_order.yml`                    | [`seed`] — append-only          |
//! | SQL seed files                      | [`seed`] — preserve after marker |
//! | Any `.rs` file with `// <<< CUSTOM` | [`custom_blocks`] — anchor + placement heuristic |
//!
//! Each strategy lives in its own submodule with its own tests. This module
//! only re-exports the entry points called from the generate pipeline.

mod custom_blocks;
mod seed;
mod yaml_config;

pub(super) use custom_blocks::{detect_unprotected_custom_code, merge_rust_mod_custom};
pub(super) use seed::{merge_seed_file, merge_seed_order};
pub(super) use yaml_config::merge_yaml_config;
