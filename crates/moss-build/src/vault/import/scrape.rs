//! Website scraping: the inverse of moss's generator.
//!
//! Generator: markdown files → HTML website. Scraper: HTML website →
//! markdown files. [`run`] is the pipeline; the modules under it are its
//! stages — [`scope`] decides what is in bounds, [`crawler`] finds the next
//! URLs, [`extractor`] and [`converter`] turn a page into content,
//! [`metadata`] and [`service`] shape the frontmatter, [`writer`] decides
//! where the file lands, and [`mhtml`] reads a local capture instead of the
//! network.
//!
//! Sibling to the dialect importers in the parent module: this half fetches
//! a live site, that half reads a site builder's own export.

// Public because a consumer outside this crate names them: `run` carries the
// engine entry point and its two DTOs, `service` the config, `converter` the
// article extractor the fixture suite exercises. The stages below are the
// pipeline's own business.
pub mod converter;
pub mod run;
pub mod service;

pub(crate) mod crawler;
pub(crate) mod extractor;
pub(crate) mod metadata;
pub(crate) mod mhtml;
pub(crate) mod scope;
pub(crate) mod writer;
