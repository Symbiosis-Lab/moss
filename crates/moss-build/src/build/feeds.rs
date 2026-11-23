//! Syndication and discovery outputs: RSS and JSON feeds, calendar files, sitemaps, llms.txt, redirects, and
//! the full-text search index.

pub mod calendar;
pub mod json_feed;
pub(crate) mod import_addresses;
pub(crate) mod kept_addresses;
pub mod llms_txt;
pub mod previews;
pub mod redirect_table;
pub mod redirects;
pub mod robots;
pub mod rss;
pub mod search;
pub mod search_lane;
pub mod sitemap;
pub(crate) mod site_files;
