//! The crate's integration tests, one binary: each file is a module here.
//! A file that sets process-wide environment (MOSS_HOME and friends) is a
//! top-level tests/*.rs, keeps its own process and is named in OWN_PROCESS.
//! `layout` fails on a file either list misses or a name without a file.
const OWN_PROCESS: &[&str] = &[
    "folder_build_lock_test",
    "folder_owner_record_test",
    "standby_takeover_test",
    "stdin_stop_test",
    "sweep_reconcile_test",
    "yield_sweep_and_standby_sigint_test",
    "yield_test",
];

mod agent_write_scope_test;
mod booking_offer_test;
mod build_default_folder_test;
mod build_json_output_test;
mod build_summary_line_test;
mod closed_output_pipe_test;
mod comment_refresh_test;
mod comments_cli_test;
mod data_file_list_test;
mod dead_link_list_test;
mod declared_fields_test;
mod deleted_page_preview_test;
mod deploy_direct_target_test;
mod deploy_site_dir_test;
mod deploy_site_files_test;
mod describe_country_map_test;
mod describe_hosts_test;
mod dot_named_site_test;
mod edit_cli_test;
mod edit_service_test;
mod fence_warnings_test;
mod field_end_test;
mod folder_icon_test;
mod folder_rename_forward_test;
mod folder_trust_cli_test;
mod folder_web_address_test;
mod footer_sections_test;
mod guide_list_test;
mod heading_attributes_test;
mod history_save_cli_test;
mod import_folder_cli_test;
mod invalid_config_build_test;
mod link_only_partial_nav_test;
mod listing_dates_test;
mod listing_style_test;
mod live_record_strict_test;
mod locator_placement_test;
mod logging_test;
mod map_layers_build_test;
mod map_layers_tiles_test;
mod migrate_cli_test;
mod min_moss_version_test;
mod move_asks_rebuild_test;
mod nested_fence_test;
mod not_found_page_test;
mod output_collision_test;
mod panel_none_strict_test;
mod partial_pages_test;
mod picture_viewer_test;
mod plain_tab_reload_test;
mod plugin_hash_cli_test;
mod rename_help_test;
mod render_cli_test;
mod request_forms_test;
mod root_cli_test;
mod save_asks_rebuild_test;
mod self_contained_test;
mod served_child;
mod sign_in_names_command_line_test;
mod site_settings_warning_test;
mod syndication_cli_test;
mod template_check_test;
mod term_editions_test;
mod title_edit_rebuild_test;
mod unresolved_embed_strict_test;
mod unwritten_site_exit_test;
mod watch_during_picture_batch_test;
#[path = "../../../moss-core/tests/it/layout.rs"]
mod layout;
