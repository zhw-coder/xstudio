pub mod commands;
pub mod context;
pub mod dto;
pub mod error;
pub mod infra;
pub mod models;
pub mod services;

/// 启动 Tauri 桌面应用。
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            tauri::async_runtime::block_on(async {
                let app_dir = infra::app_dir()?;
                plugins::PluginRuntime::global_with_options(plugins::PluginRuntimeOptions {
                    app_dir,
                })
                .map_err(|error| std::io::Error::other(error.to_string()))?;
                services::config::init(app.handle()).await?;
                services::search::init(app.handle()).await
            })
            .map_err(|error| {
                eprintln!("初始化运行时配置失败: {error:?}");
                Box::new(error) as Box<dyn std::error::Error>
            })?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app::open_external_url,
            commands::chat::list_session_repos,
            commands::chat::list_chat_sessions,
            commands::chat::open_chat_session,
            commands::chat::create_chat_session,
            commands::chat::fork_chat_session,
            commands::chat::delete_chat_session,
            commands::chat::prompt_chat,
            commands::chat::abort_chat,
            commands::chat::resolve_chat_tool_approval,
            commands::chat::list_chat_resources_names,
            commands::chat::compact_chat,
            commands::chat::withdraw_chat_turn,
            commands::chat::edit_and_prompt_chat_user_message,
            commands::chat::skill_chat,
            commands::chat::template_chat,
            commands::chat::set_chat_stream_options,
            commands::chat::set_chat_model,
            commands::chat::set_chat_thinking_level,
            commands::chat::set_chat_tools,
            commands::chat::set_chat_session_name,
            commands::config::get_config,
            commands::config::set_config,
            commands::config::open_app_dir,
            commands::resources::list_template_files,
            commands::resources::list_skill_files,
            commands::resources::set_skill_disable_model_invocation,
            commands::resources::get_template_file,
            commands::resources::save_template_file,
            commands::resources::delete_template_file,
            commands::project::list_projects,
            commands::project::save_project,
            commands::project::delete_project,
            commands::models::list_providers,
            commands::models::create_provider,
            commands::models::update_provider,
            commands::models::delete_provider,
            commands::models::list_models_by_provider,
            commands::models::sync_models_by_provider,
            commands::models::all_provider_models_map,
            commands::models::provider_model_ids_map,
            commands::models::fetch_models_from_provider,
            commands::models::model_thinking_levels,
            commands::models::set_model_record_selection,
            commands::models::set_model_thinking_level,
            commands::models::list_tool_names,
            commands::models::add_preference_tool,
            commands::models::remove_preference_tool,
            commands::models::set_preference_tools,
            commands::models::set_preference_tool_enabled,
            commands::models::set_preference_approval,
            commands::search::list_search_engines,
            commands::search::get_search_engine,
            commands::search::list_search_configs,
            commands::search::save_search_config
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|error| {
            eprintln!("Tauri 应用运行失败: {error:?}");
        });
}
