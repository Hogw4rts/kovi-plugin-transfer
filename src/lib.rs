#![deny(clippy::correctness)]
#![warn(clippy::suspicious, clippy::style, clippy::complexity, clippy::perf)]

mod cmd;
mod config;
mod forward;
mod media;

use config::load_config;
use kovi::log::info;
use kovi::PluginBuilder as plugin;
use std::path::PathBuf;
use std::sync::Arc;

pub(crate) static DATA_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

#[kovi::plugin]
async fn main() {
    let bot = plugin::get_runtime_bot();
    let data_path = DATA_PATH.get_or_init(|| bot.get_data_path()).clone();
    let data_path = Arc::new(data_path);

    let config = Arc::new(tokio::sync::RwLock::new(load_config(&data_path)));

    info!("transfer: plugin loaded, {} rules", config.read().await.rules.len());
    info!("transfer: commands: /transfer add|remove|list|enable|disable");

    plugin::on_msg({
        let bot = bot.clone();
        let config = config.clone();
        let data_path = data_path.clone();

        move |event| {
            let bot = bot.clone();
            let config = config.clone();
            let data_path = data_path.clone();

            async move {
                let text = event.borrow_text().map(|t| t.trim().to_string()).unwrap_or_default();
                let user_id = event.sender.user_id;

                if text.starts_with("/transfer") {
                    if !bot.get_all_admin().map_or(false, |a| a.contains(&user_id)) {
                        return;
                    }

                    let parts: Vec<&str> = text.split_whitespace().collect();
                    if parts.len() < 2 {
                        event.reply("用法:\n\
                            /transfer add <private|group|group_user> <id> [id] -> <private|group> <id>\n\
                            /transfer remove <index>\n\
                            /transfer list\n\
                            /transfer enable <index>\n\
                            /transfer disable <index>");
                        return;
                    }

                    cmd::handle_command(&parts, &config, &data_path, &event).await;
                    return;
                }

                if text.starts_with('/') {
                    return;
                }

                let self_id = event.self_id;
                if user_id == self_id {
                    return;
                }

                let rules = { config.read().await.rules.clone() };
                forward::forward_message(&bot, &event, &rules, self_id).await;
            }
        }
    });
}
