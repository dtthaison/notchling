pub mod actions;
mod actions_util;
pub mod address;
pub mod addresses;
pub mod admin_secret;
pub mod catalog;
pub mod device_login;
pub mod dist_manifest;
pub mod engine;
pub mod engine_check;
pub mod engine_fix;
pub mod engine_plan;
mod engine_update;
pub mod forms;
pub mod github;
pub mod host_trust;
pub mod mac_facts;
pub mod masks;
pub mod merge;
pub mod merge_ops;
pub mod morning;
pub mod names;
pub mod node_updates;
pub mod offers;
pub mod poller;
pub mod process;
pub mod reach;
pub mod registry;
pub mod release_download;
pub mod remote_edit;
pub mod repo_common;
pub mod roots;
pub mod runner;
pub mod service_files;
pub mod shell;
pub mod specs;
pub mod ssh_command;
pub mod ssh_key;
pub mod ssh_test;
pub mod status;
pub mod step;
mod steps_git;
pub mod steps_machine;
pub mod steps_master;
pub mod steps_node;
mod steps_node_binary;
mod steps_node_logins;
mod steps_node_mac;
pub mod steps_node_reach;
mod steps_node_service;
pub mod steps_push;
pub mod steps_replace;
pub mod steps_repo;
pub mod steps_sd;
pub mod steps_sd_check;
pub mod steps_sd_service;
pub mod steps_secret;
pub mod steps_selfupdate;
pub mod steps_tailscale_linux;
mod steps_versions;
mod steps_xcode;
pub mod tailscale;
pub mod token_creation;
pub mod token_scope;
mod token_scope_net;
mod tool_guard;
pub mod tool_installs;
pub mod tool_login;
pub mod tool_updates;
pub mod usage;
pub mod versions_facts;
pub mod worker_json;

#[cfg(test)]
pub mod fakes;
#[cfg(test)]
pub mod fakes_b;
#[cfg(test)]
pub mod fakes_jobs;
#[cfg(test)]
mod tests_actions;
#[cfg(test)]
mod tests_actions_files;
#[cfg(test)]
mod tests_actions_gh;
#[cfg(test)]
mod tests_actions_read;
#[cfg(test)]
mod tests_address;
#[cfg(test)]
mod tests_addresses;
#[cfg(test)]
mod tests_admin_vault;
#[cfg(test)]
mod tests_device_login;
#[cfg(test)]
mod tests_dist_manifest;
#[cfg(test)]
mod tests_engine;
#[cfg(test)]
mod tests_engine_check;
#[cfg(test)]
mod tests_engine_fix;
#[cfg(test)]
mod tests_engine_offers;
#[cfg(test)]
pub mod tests_engine_world;
#[cfg(test)]
mod tests_engine_writes;
#[cfg(test)]
mod tests_forms;
#[cfg(test)]
mod tests_host_trust;
#[cfg(test)]
mod tests_machine_steps;
#[cfg(test)]
mod tests_machine_tools;
#[cfg(test)]
mod tests_master_steps;
#[cfg(test)]
mod tests_merge;
#[cfg(test)]
mod tests_merge_b;
#[cfg(test)]
mod tests_merge_c;
#[cfg(all(test, unix))]
mod tests_merge_git;
#[cfg(all(test, unix))]
mod tests_merge_git_b;
#[cfg(test)]
mod tests_morning;
#[cfg(test)]
mod tests_node_app;
#[cfg(test)]
mod tests_node_final;
#[cfg(test)]
mod tests_node_join;
#[cfg(test)]
mod tests_node_keychain;
#[cfg(test)]
mod tests_node_logins;
#[cfg(test)]
mod tests_node_mac;
#[cfg(test)]
mod tests_node_profile;
#[cfg(test)]
mod tests_node_steps;
#[cfg(test)]
mod tests_node_steps_b;
#[cfg(test)]
mod tests_node_updates;
#[cfg(test)]
mod tests_poller;
#[cfg(test)]
mod tests_reach;
#[cfg(test)]
mod tests_reach_short;
#[cfg(test)]
mod tests_registry;
#[cfg(test)]
mod tests_release_download;
#[cfg(test)]
mod tests_remote_edit;
#[cfg(test)]
mod tests_repo;
#[cfg(test)]
mod tests_repo_push;
#[cfg(test)]
mod tests_repo_replace;
#[cfg(test)]
mod tests_repo_replace_b;
#[cfg(all(test, unix))]
mod tests_repo_shell;
#[cfg(test)]
mod tests_routing;
#[cfg(all(test, unix))]
mod tests_runner;
#[cfg(test)]
mod tests_safety;
#[cfg(test)]
mod tests_safety_b;
#[cfg(test)]
mod tests_safety_c;
#[cfg(test)]
mod tests_sd;
#[cfg(test)]
mod tests_sd_check;
#[cfg(test)]
mod tests_sd_service;
#[cfg(test)]
mod tests_sd_service_b;
#[cfg(test)]
mod tests_sd_service_versions;
#[cfg(test)]
mod tests_sd_worker;
#[cfg(test)]
mod tests_secret_node;
#[cfg(test)]
mod tests_secret_steps;
#[cfg(test)]
mod tests_secrets;
#[cfg(test)]
mod tests_selfupdate;
#[cfg(test)]
mod tests_shell;
#[cfg(test)]
mod tests_specs;
#[cfg(test)]
mod tests_ssh_command;
#[cfg(test)]
mod tests_ssh_key;
#[cfg(test)]
mod tests_ssh_test;
#[cfg(test)]
mod tests_status;
#[cfg(test)]
mod tests_tailscale;
#[cfg(test)]
mod tests_tailscale_linux;
#[cfg(test)]
mod tests_token_scope;
#[cfg(test)]
mod tests_token_scope_gh;
#[cfg(test)]
mod tests_token_scope_view;
#[cfg(test)]
mod tests_tool_installs;
#[cfg(test)]
mod tests_tool_login;
#[cfg(test)]
mod tests_tool_updates;
#[cfg(test)]
mod tests_versions_facts;
#[cfg(test)]
pub(crate) mod tests_versions_probe;
