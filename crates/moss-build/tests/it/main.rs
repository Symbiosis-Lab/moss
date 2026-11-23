//! The crate's integration tests, one binary: each file is a module here.
//! A file that installs a global logger, sets process environment, keeps its
//! own build lock or asserts on process-wide state is a top-level tests/*.rs,
//! keeps its own process and is named in OWN_PROCESS. `layout` fails on a file
//! either list misses or a name without a file.
const OWN_PROCESS: &[&str] = &[
    "app_identity_test",
    "article_end_block_test",
    "booking_step_aside_test",
    "child_listing_repeat_note_test",
    "colophon_credit_test",
    "country_map_test",
    "declarative_contributions_test",
    "diagram_warning_test",
    "first_picture_cover_test",
    "folder_trust_test",
    "ghost_import_test",
    "hash_index_tally_test",
    "hero_caption_links_test",
    "image_alt_warning_test",
    "image_display_words_test",
    "image_inline_width_note_test",
    "incremental_site_oracle",
    "incremental_surface_equivalence",
    "link_preview_setting_test",
    "luminance_lazy_test",
    "members_public_test",
    "nav_external_url_test",
    "no_plugins_problem_count_test",
    "no_system_ca_test",
    "parse_cache_kept_test",
    "passthrough_markdown_test",
    "place_map_pack_test",
    "plugin_admission_test",
    "plugin_install_cli_test",
    "proved_trigger_test",
    "registry_status_test",
    "section_color_test",
    "seta_poll_log_test",
    "shortcode_attr_warning_test",
    "snapshot_tests",
    "unknown_place_test",
    "untrusted_folder_identity_test",
    "welcome_email_test",
    "wordpress_import_test",
];

mod accordion_callouts_test;
mod ai_switches_test;
mod archival_closing_picture_test;
mod bare_year_date_test;
mod beacon_file_test;
mod body_heading_replaces_title_note_test;
mod breadcrumb_separator_contrast_test;
mod build_tree_migration_test;
mod card_fields_test;
mod card_subtitle_test;
mod card_text_wrap_test;
mod cascade_author_test;
mod channel_choice_test;
mod closing_picture_theme_note_test;
mod colophon_switch_test;
mod comment_moderation_gate_test;
mod comment_refresh_on_request_test;
mod composed_asset_name_test;
mod copy_dir;
mod country_map_contrast_test;
mod country_map_search_test;
mod cross_edition_listing_test;
mod declared_name_list_test;
mod derived_embed_test;
mod edition_home_listing_test;
mod embed_forms_test;
mod embed_more_none_test;
mod engine_lock_invariant_test;
mod escaping_symlink_note_test;
mod event_when_test;
mod external_url_page_test;
mod filename_date_test;
mod first_build_media_test;
mod folder_embed_indexless_test;
mod folder_redirect_test;
mod folder_url_embed_test;
mod folder_when_test;
mod folder_wikilink_parity_test;
mod footer_closing_picture_test;
mod footer_touch_target_test;
mod gallery_viewer_test;
mod german_edition_test;
mod guide_content_selectors_test;
mod hardlink_invariant_test;
mod header_scroll_test;
mod hero_from_test;
mod hero_listing_test;
mod hero_poster_test;
mod hls_ladder_shape_test;
mod home_h1_test;
mod home_map_nav_note_test;
mod home_tagline_test;
mod image_title_test;
mod img_contract_test;
mod inline_subscribe_scope_test;
mod interchangeable_forms_test;
mod json_feed_test;
mod lead_listing_test;
mod legacy_moss_ignore_note_test;
mod list_translations_test;
mod listing_classes_test;
mod listing_holds_test;
mod listing_order_across_styles_test;
mod listing_picture_strip_test;
mod listing_same_date_weight_test;
mod map_terms_test;
mod mark_sync_test;
mod marquee_test;
mod math_wiring_invariant_test;
mod missing_marked_picture_test;
mod nav_island_rule_test;
mod nav_partial_language_test;
mod newsletter_config_test;
mod no_string_emits;
mod no_tokio_spawn_in_tauri_context_test;
mod no_vendor_ui_launch_test;
mod one_resolver_test;
mod opening_picture_banner_test;
mod output_write_invariant_test;
mod own_listing_test;
mod page_chrome_none_test;
mod page_owned_assets_test;
mod page_subtitle_test;
mod passthrough_listing_alias_test;
mod place_dot_links_test;
mod place_without_coordinates_test;
mod places_explorer_synthetic_root_test;
mod places_line_switch_test;
mod plain_description_test;
mod publish_gate_invariant_test;
mod publish_record_ordering_test;
mod read_input_invariant_test;
mod removed_pages_test;
mod rename_boundary_test;
mod rename_home_carry_test;
mod root_identity_owner_test;
mod rust_scan;
mod same_site_everywhere_test;
mod scanned_roots_nonempty;
mod scrape_fixtures;
mod scrape_run_public_path_test;
mod selection_tools_test;
mod serve_pointer_owner_invariant_test;
mod served_path_inventory_test;
mod shadowed_index_md_test;
mod share_card_theme_test;
mod share_image_pick_test;
mod site;
mod site_archival_test;
mod site_identity_test;
mod site_logo_served_url_test;
mod skill_package_paths_test;
mod source_safety_test;
mod staging_unlink_invariant_test;
mod static_map_layer_shape_test;
mod stray_cell_divider_note_test;
mod stream_listing_test;
mod subpath_mount_test;
mod subscribe_prompt_label_test;
mod synced_siblings_test;
mod synced_writes_test;
mod syndication_line_test;
mod term_editions_test;
mod term_index_cards_test;
mod term_listing_placement_test;
mod term_root_embed_test;
mod terms_nav_test;
mod theme_asset_hash_test;
mod theme_modules_test;
mod theme_token_override_test;
mod token_declared_test;
mod token_emit_test;
mod translation_pairing_by_path_test;
mod unbuilt_link_target_test;
mod unpaired_rename_identity_test;
mod url_folder_removal_test;
mod watch_rebuild_stand_in_test;
mod wiki_embed_alt_only_test;
#[path = "../../../moss-core/tests/it/layout.rs"]
mod layout;
