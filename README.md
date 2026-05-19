# kovi-plugin-transfer

Kovi 插件 — 将 QQ 消息从指定输入源转发到指定输出目标，支持私聊和群聊，管理员可动态管理转发规则。

## 功能

- **消息转发** — 根据配置的规则，将来源消息转发到目标
- **多种来源** — 私聊消息、群聊全部消息、群聊指定用户消息
- **多种目标** — 私聊发送、群聊发送
- **QQ 指令** — `/transfer add|remove|list|enable|disable`，管理员可用
- **消息段支持** — 转发文本、图片、@提及、回复、表情
- **持久化存储** — 规则存储在 `transfer.json`，重启不丢失
- **防循环** — 自动跳过转发给自己的消息、源和目标相同的规则

## 安装

在 `Cargo.toml` 中添加：

```toml
[dependencies]
kovi-plugin-transfer = { git = "https://github.com/Hogw4rts/kovi-plugin-transfer.git" }
```

然后在 `main.rs` 中引入：

```rust
use kovi::build_bot;
use kovi_plugin_transfer;

fn main() {
    let bot = build_bot!(kovi_plugin_transfer);
    bot.run();
}
```

## 命令

所有命令仅管理员可用。

| 命令 | 说明 |
|------|------|
| `/transfer` | 查看用法 |
| `/transfer add <源类型> <id...> -> <目标类型> <id>` | 添加转发规则 |
| `/transfer remove <索引>` | 移除规则 |
| `/transfer list` | 列出所有规则 |
| `/transfer enable <索引>` | 启用规则 |
| `/transfer disable <索引>` | 禁用规则 |

### 源类型

| 类型 | 语法 | 说明 |
|------|------|------|
| `private` | `private <qq>` | 来自指定 QQ 的私聊消息 |
| `group` | `group <群号>` | 来自指定群的全部消息 |
| `group_user` | `group_user <群号> <qq>` | 来自指定群中指定 QQ 的消息 |

### 目标类型

| 类型 | 语法 | 说明 |
|------|------|------|
| `private` | `private <qq>` | 发送私聊到指定 QQ |
| `group` | `group <群号>` | 发送到指定群 |

### 示例

```
# 将 QQ 123456 的私聊转发到群 789012
/transfer add private 123456 -> group 789012

# 将群 789012 中 QQ 111111 的消息转发给 QQ 222222
/transfer add group_user 789012 111111 -> private 222222

# 将群 789012 的全部消息转发到群 333333
/transfer add group 789012 -> group 333333

# 查看所有规则
/transfer list

# 禁用第 1 条规则
/transfer disable 1
```

## 配置

配置文件位于 `data/kovi-plugin-transfer/transfer.json`，首次运行时自动生成。

```json
{
  "rules": [
    {
      "enabled": true,
      "name": "private 123456 -> group 789012",
      "source": { "type": "private", "qq": 123456 },
      "target": { "type": "group", "group_id": 789012 }
    },
    {
      "enabled": true,
      "name": "group_user 789012 111111 -> private 222222",
      "source": { "type": "group_user", "group_id": 789012, "qq": 111111 },
      "target": { "type": "private", "qq": 222222 }
    }
  ]
}
```

## 转发格式

转发消息会在开头添加来源标签：

```
[转发自私聊 - 张三(123456)]
你好，请帮我转发这条消息

[转发自群 789012 - 李四(111111)]
这是一条群聊消息
```

消息中的图片、@提及、回复引用、表情等消息段也会一并转发。

## 架构

```
QQ 消息
  │
  ▼
Kovi on_msg
  │
  ├─ 匹配是否 /transfer 命令 ──▶ 管理规则 (add/remove/list/enable/disable)
  │
  └─ 遍历所有规则 ──▶ 源匹配 ──▶ 构建转发消息 ──▶ 发送到目标
       │
       ├─ 跳过自身消息
       ├─ 跳过源=目标 (防循环)
       └─ 转发文本 + 图片 + @ + 回复 + 表情
```

## 安全

- **管理员权限**：所有管理命令仅限 Kovi 框架中配置的管理员
- **防循环**：自动跳过源和目标相同的规则（如群 A 转发到群 A）、转发给自身的规则、转发给发送者的规则
- **命令拦截**：以 `/transfer` 开头的消息不会被转发

## License

GPL-3.0
