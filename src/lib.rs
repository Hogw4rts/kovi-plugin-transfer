mod config;

use config::{Source, Target, TransferRule, load_config, save_config};
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
                let text = match event.borrow_text() {
                    Some(t) => t.trim().to_string(),
                    None => return,
                };

                let user_id = event.sender.user_id;

                if text.starts_with("/transfer") {
                    match bot.get_all_admin() {
                        Ok(admins) => {
                            if !admins.contains(&user_id) {
                                return;
                            }
                        }
                        Err(_) => return,
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

                    match parts[1] {
                        "add" => {
                            let mut cfg = config.write().await;
                            if let Some(rule) = parse_add_rule(&parts) {
                                cfg.rules.push(rule);
                                save_config(&cfg, &data_path);
                                event.reply(format!("[OK] 已添加规则 #{}, 共 {} 条规则", cfg.rules.len(), cfg.rules.len()));
                            } else {
                                event.reply("格式: /transfer add <private|group|group_user> <id1> [id2] -> <private|group> <id>");
                            }
                        }
                        "remove" | "del" | "delete" => {
                            let mut cfg = config.write().await;
                            if let Some(idx) = parts.get(2).and_then(|s| s.parse::<usize>().ok()) {
                                if idx < 1 || idx > cfg.rules.len() {
                                    event.reply(format!("索引无效: {}（有效范围 1-{}）", idx, cfg.rules.len()));
                                } else {
                                    let removed = cfg.rules.remove(idx - 1);
                                    save_config(&cfg, &data_path);
                                    event.reply(format!("[OK] 已移除规则 #{} ({})", idx, describe_rule(&removed, idx)));
                                }
                            } else {
                                event.reply("用法: /transfer remove <索引>");
                            }
                        }
                        "list" => {
                            let cfg = config.read().await;
                            if cfg.rules.is_empty() {
                                event.reply("暂无转发规则。");
                                return;
                            }
                            let mut lines = vec!["转发规则列表:".to_string()];
                            for (i, rule) in cfg.rules.iter().enumerate() {
                                lines.push(describe_rule(rule, i + 1));
                            }
                            event.reply(lines.join("\n"));
                        }
                        "enable" | "on" => {
                            let mut cfg = config.write().await;
                            if let Some(idx) = parts.get(2).and_then(|s| s.parse::<usize>().ok()) {
                                if idx < 1 || idx > cfg.rules.len() {
                                    event.reply(format!("索引无效: {}（有效范围 1-{}）", idx, cfg.rules.len()));
                                } else {
                                    cfg.rules[idx - 1].enabled = true;
                                    save_config(&cfg, &data_path);
                                    event.reply(format!("[OK] 已启用规则 #{}", idx));
                                }
                            } else {
                                event.reply("用法: /transfer enable <索引>");
                            }
                        }
                        "disable" | "off" => {
                            let mut cfg = config.write().await;
                            if let Some(idx) = parts.get(2).and_then(|s| s.parse::<usize>().ok()) {
                                if idx < 1 || idx > cfg.rules.len() {
                                    event.reply(format!("索引无效: {}（有效范围 1-{}）", idx, cfg.rules.len()));
                                } else {
                                    cfg.rules[idx - 1].enabled = false;
                                    save_config(&cfg, &data_path);
                                    event.reply(format!("[OK] 已禁用规则 #{}", idx));
                                }
                            } else {
                                event.reply("用法: /transfer disable <索引>");
                            }
                        }
                        _ => event.reply("未知子命令。可用: add, remove, list, enable, disable"),
                    }
                    return;
                }

                if text.starts_with('/') {
                    return;
                }

                let self_id = event.self_id;
                if user_id == self_id {
                    return;
                }

                let cfg = config.read().await;
                forward_message(&bot, &event, &cfg.rules, self_id).await;
            }
        }
    });
}

fn parse_add_rule(parts: &[&str]) -> Option<TransferRule> {
    let arrow_pos = parts.iter().position(|&p| p == "->")?;
    if arrow_pos < 3 || arrow_pos + 3 > parts.len() {
        return None;
    }

    let source = parse_source(&parts[2..arrow_pos])?;
    let target = parse_target(&parts[arrow_pos + 1..])?;

    let name = format!(
        "{} {} -> {} {}",
        parts[2], parts[3..arrow_pos].join(" "),
        parts[arrow_pos + 1], parts[arrow_pos + 2..].join(" ")
    );

    Some(TransferRule {
        enabled: true,
        name,
        source,
        target,
    })
}

fn parse_source(parts: &[&str]) -> Option<Source> {
    match parts[0] {
        "private" => {
            let qq = parts.get(1)?.parse().ok()?;
            Some(Source::Private { qq })
        }
        "group" => {
            let group_id = parts.get(1)?.parse().ok()?;
            Some(Source::Group { group_id })
        }
        "group_user" => {
            let group_id = parts.get(1)?.parse().ok()?;
            let qq = parts.get(2)?.parse().ok()?;
            Some(Source::GroupUser { group_id, qq })
        }
        _ => None,
    }
}

fn parse_target(parts: &[&str]) -> Option<Target> {
    match parts[0] {
        "private" => {
            let qq = parts.get(1)?.parse().ok()?;
            Some(Target::Private { qq })
        }
        "group" => {
            let group_id = parts.get(1)?.parse().ok()?;
            Some(Target::Group { group_id })
        }
        _ => None,
    }
}

fn describe_source(source: &Source) -> String {
    match source {
        Source::Private { qq } => format!("私聊 QQ {}", qq),
        Source::Group { group_id } => format!("群 {} 全部消息", group_id),
        Source::GroupUser { group_id, qq } => format!("群 {} 中 QQ {} 的消息", group_id, qq),
    }
}

fn describe_target(target: &Target) -> String {
    match target {
        Target::Private { qq } => format!("私聊 QQ {}", qq),
        Target::Group { group_id } => format!("群 {}", group_id),
    }
}

fn describe_rule(rule: &TransferRule, index: usize) -> String {
    let status = if rule.enabled { "[ON]" } else { "[OFF]" };
    let label = if rule.name.is_empty() {
        String::new()
    } else {
        format!(" ({})", rule.name)
    };
    format!(
        "  #{}. {} {} -> {}{}",
        index,
        status,
        describe_source(&rule.source),
        describe_target(&rule.target),
        label
    )
}

async fn forward_message(
    bot: &kovi::RuntimeBot,
    event: &kovi::MsgEvent,
    rules: &[TransferRule],
    self_id: i64,
) {
    if rules.is_empty() {
        return;
    }

    let msg_type = if event.group_id.is_some() { "group" } else { "private" };
    let user_id = event.sender.user_id;
    let group_id = event.group_id.unwrap_or(0);
    let sender_name = event.sender.nickname.clone().unwrap_or_default();

    for rule in rules {
        if !rule.enabled {
            continue;
        }

        let matched = match &rule.source {
            Source::Private { qq } => *qq == user_id && msg_type == "private",
            Source::Group { group_id: gid } => *gid == group_id && msg_type == "group",
            Source::GroupUser { group_id: gid, qq } => {
                *gid == group_id && *qq == user_id && msg_type == "group"
            }
        };

        if !matched {
            continue;
        }

        if matches!(&rule.target, Target::Private { qq } if *qq == self_id) {
            continue;
        }

        if matches!(&rule.target, Target::Group { group_id: gid } if *gid == group_id) {
            continue;
        }

        if matches!(&rule.target, Target::Private { qq } if *qq == user_id) {
            continue;
        }

        let label = build_forward_label(&rule.source, &sender_name, user_id);

        send_forward(bot, event, &rule.target, &label).await;
    }
}

fn build_forward_label(source: &Source, sender_name: &str, user_id: i64) -> String {
    match source {
        Source::Private { .. } => {
            format!("[转发自私聊 - {}({})]\n", sender_name, user_id)
        }
        Source::Group { group_id } => {
            format!("[转发自群 {} - {}({})]\n", group_id, sender_name, user_id)
        }
        Source::GroupUser { group_id, .. } => {
            format!("[转发自群 {} - {}({})]\n", group_id, sender_name, user_id)
        }
    }
}

async fn send_forward(
    bot: &kovi::RuntimeBot,
    event: &kovi::MsgEvent,
    target: &Target,
    label: &str,
) {
    let mut msg = kovi::Message::new().add_text(label);

    for seg in &event.message.get("text") {
        if let Some(text) = seg.data.get("text").and_then(|v| v.as_str()) {
            if !text.trim().is_empty() {
                msg = msg.add_text(text);
            }
        }
    }

    for seg in &event.message.get("image") {
        if let Some(url) = seg.data.get("url").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            msg = msg.add_image(url);
        } else if let Some(file) = seg.data.get("file").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            msg = msg.add_image(file);
        }
    }

    for seg in &event.message.get("at") {
        if let Some(qq) = seg.data.get("qq").and_then(|v| v.as_str()) {
            msg = msg.add_at(qq);
        }
    }

    for seg in &event.message.get("reply") {
        if let Some(id) = seg.data.get("id") {
            if let Some(id) = id.as_i64() {
                msg = msg.add_reply(id as i32);
            } else if let Some(id) = id.as_str().and_then(|s| s.parse::<i32>().ok()) {
                msg = msg.add_reply(id);
            }
        }
    }

    for seg in &event.message.get("face") {
        if let Some(id) = seg.data.get("id") {
            if let Some(id) = id.as_i64() {
                msg = msg.add_face(id);
            } else if let Some(id) = id.as_str().and_then(|s| s.parse::<i64>().ok()) {
                msg = msg.add_face(id);
            }
        }
    }

    match target {
        Target::Private { qq } => {
            bot.send_private_msg(*qq, msg);
            info!(
                "transfer: forwarded private {} -> private {}",
                event.sender.user_id, qq
            );
        }
        Target::Group { group_id } => {
            bot.send_group_msg(*group_id, msg);
            info!(
                "transfer: forwarded -> group {}",
                group_id
            );
        }
    }
}

