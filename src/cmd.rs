use crate::config::{Source, Target, TransferConfig, TransferRule, save_config};

use std::path::Path;
use std::sync::Arc;

pub(crate) async fn handle_command(
    parts: &[&str],
    config: &Arc<tokio::sync::RwLock<TransferConfig>>,
    data_path: &Path,
    event: &kovi::MsgEvent,
) -> bool {
    match parts[1] {
        "add" => {
            let mut cfg = config.write().await;
            if let Some(rule) = parse_add_rule(parts) {
                cfg.rules.push(rule);
                save_config(&cfg, data_path);
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
                    save_config(&cfg, data_path);
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
                return true;
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
                    save_config(&cfg, data_path);
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
                    save_config(&cfg, data_path);
                    event.reply(format!("[OK] 已禁用规则 #{}", idx));
                }
            } else {
                event.reply("用法: /transfer disable <索引>");
            }
        }
        _ => event.reply("未知子命令。可用: add, remove, list, enable, disable"),
    }
    true
}

pub(crate) fn parse_add_rule(parts: &[&str]) -> Option<TransferRule> {
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

pub(crate) fn describe_source(source: &Source) -> String {
    match source {
        Source::Private { qq } => format!("私聊 QQ {}", qq),
        Source::Group { group_id } => format!("群 {} 全部消息", group_id),
        Source::GroupUser { group_id, qq } => format!("群 {} 中 QQ {} 的消息", group_id, qq),
    }
}

pub(crate) fn describe_target(target: &Target) -> String {
    match target {
        Target::Private { qq } => format!("私聊 QQ {}", qq),
        Target::Group { group_id } => format!("群 {}", group_id),
    }
}

pub(crate) fn describe_rule(rule: &TransferRule, index: usize) -> String {
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

pub(crate) fn build_forward_label(source: &Source, sender_name: &str, user_id: i64) -> String {
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
