//! moss HTML/CSS contract surface.
//!
//! Federated sources for the moss design contract:
//! - [`tokens`]: W3C Design Tokens (CSS variables, defaults, groups, descriptions)
//! - [`components`]: emitter contract — class names, data attributes, examples
//!
//! Consumed by the codegen binary at `src-tauri/dev-bin/generate-artifacts.rs`,
//! by `moss describe` (Phase 2), and by snapshot tests.

pub mod tokens;
pub mod components;
pub mod custom_props;
pub mod describe;
pub mod frontmatter;
pub mod shortcodes;
pub mod sizes;
