//! The crate's integration tests, one binary: each file is a module here.
//! A new test file is a line below and must exist in this directory; `layout`
//! fails on a file this list misses or a name here without a file.
mod attr_value_render;
mod components_sync_test;
mod components_test;
mod describe_test;
mod diagram_hook;
mod folder_embed_params;
mod footnotes_and_strikethrough;
mod img_contract_test;
mod math_delimiters;
mod math_parsing;
mod math_wiring_invariant_test;
mod never_panics;
mod parser_parity;
mod picture_words;
mod position_vectors;
mod shortcode_structure_parity;
mod tokens_test;
mod layout;
