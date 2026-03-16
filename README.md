# GPTGO

`GPTGO` 是一个基于 `Tauri + Rust + TypeScript` 的桌面应用，用来替代原先的 Python Web UI 版本，目标是把账号注册、邮件通道管理、账号管理、数据库管理和运行日志收敛到一个本地桌面工具里。

当前仓库是 Rust 重写版，重点放在：

- 本地桌面端运行，不依赖 Python WebUI
- 支持 `GPTMail` 和 `自定义邮箱 API` 两类邮件通道
- 支持“先预生成邮箱，确认后再执行注册”的工作流
- 支持单次注册、批量注册、账号管理、数据库管理、代理设置和部分全局设置

这不是一个通用 SDK，而是一个面向桌面端操作和本地运行的工具型应用。

## 当前状态

当前 Rust 版已经实现：

- 桌面应用壳：`Tauri + Vite + TypeScript`
- 邮件通道：
  - `GPTMail`
  - `自定义邮箱 API`
- 注册工作流：
  - 预生成邮箱
  - 确认后执行
  - 单次注册
  - 批量注册
- 运行时功能：
  - 任务状态追踪
  - 实时任务事件推送（桌面端等价于 WebSocket 推送）
  - 右侧实时日志面板
- 设置功能：
  - 邮件通道配置
  - 代理配置
  - OpenAI 相关全局参数
  - 注册参数
  - CPA 参数
- 账号管理：
  - 列表展示
  - 批量删除
  - 状态更新
  - 导出 JSON / CSV / CPA
  - Token 刷新
  - Token 验证
  - 批量验证
  - CPA 上传
- 数据库管理：
  - 数据库信息查看
  - 备份数据库
  - 清理任务记录

当前尚未实现或未完全迁移：

- `Outlook` 通道和 Outlook 批量注册
- 代理列表、动态代理 API、代理健康检查
- 更完整的服务优先级管理与复杂编辑体验
- 更丰富的数据库维护操作（例如 Vacuum、分级清理、恢复）
- 原 Python 版里更细粒度的设置项

如果你要做功能验收，建议把它当作“Rust 桌面版第一阶段可用版本”，而不是认为已经 100% 替代原 Python 版。

## 技术栈

前端：

- `TypeScript`
- `Vite`
- `@tauri-apps/api`

后端：

- `Rust`
- `Tauri`
- `reqwest`
- `rusqlite`
- `tokio`
- `serde`

本地存储：

- `SQLite`

## 目录结构

```text
codex-register-rust/
├── index.html
├── package.json
├── package-lock.json
├── tsconfig.json
├── vite.config.ts
├── src/
│   ├── main.ts
│   └── style.css
├── src-tauri/
│   ├── Cargo.toml
│   ├── Cargo.lock
│   ├── build.rs
│   ├── tauri.conf.json
│   └── src/
│       ├── main.rs
│       ├── lib.rs
│       ├── app_state.rs
│       ├── commands.rs
│       ├── db.rs
│       ├── error.rs
│       ├── models.rs
│       ├── task_manager.rs
│       ├── openai/
│       │   ├── constants.rs
│       │   ├── engine.rs
│       │   └── mod.rs
│       └── services/
│           ├── mod.rs
│           ├── gptmail.rs
│           ├── custom_domain.rs
│           ├── token.rs
│           └── cpa.rs
└── .gitignore
```

## 运行要求

建议环境：

- `Node.js >= 20`
- `npm >= 10`
- `Rust >= 1.77`
- macOS / Windows / Linux 中支持 Tauri 2 的桌面环境

本项目当前在 macOS 上做了实际运行和开发调试。

## 安装依赖

在项目根目录执行：

```bash
npm install
```

Rust 依赖由 Cargo 自动管理，不需要单独手工安装 crate。

## 开发运行

在项目根目录执行：

```bash
npm run tauri dev
```

这条命令会同时启动：

- Vite 开发服务器
- Tauri 桌面应用进程

正常情况下，桌面应用会直接打开。

## 带代理运行

如果你的注册、邮件轮询、OpenAI 请求需要走代理，可以像下面这样启动：

```bash
export https_proxy=http://127.0.0.1:7897
export http_proxy=http://127.0.0.1:7897
export all_proxy=socks5://127.0.0.1:7897

npm run tauri dev
```

如果你不想依赖环境变量，也可以在应用的“设置”页里保存代理配置。当前实现里：

- 桌面端全局代理设置会写入本地数据库
- 邮件客户端和 OpenAI 注册流程会优先读应用内设置
- 如果应用内没开代理，再回退到系统环境变量

## 生产构建

前端构建：

```bash
npm run build
```

Rust 后端检查：

```bash
cd src-tauri
cargo check
```

如需正式打包：

```bash
npm run tauri build
```

## 应用界面说明

当前界面分为三大区域：

- 左侧：
  - 应用品牌区
  - 一级功能菜单
  - 当前服务摘要
- 中间：
  - 当前功能主工作区
  - 例如注册调度、服务设置、邮件池、账号管理等
- 右侧：
  - 实时任务日志
  - 当前任务摘要

左侧菜单目前有四类：

- `注册`
- `账号`
- `邮件`
- `设置`

### 注册页

用于执行主流程：

- 查看注册概览
- 选择邮件通道
- 切换单次/批量模式
- 预生成邮箱
- 确认并开始注册
- 查看任务列表

### 账号页

用于账号管理：

- 查看账号统计
- 查看账号记录
- 批量删除
- 批量标记状态
- 导出 JSON / CSV / CPA
- 执行 Token 刷新
- 执行 Token 验证
- 批量验证
- 上传到 CPA

### 邮件页

用于查看预生成邮箱池和流程说明：

- 查看当前待确认邮箱
- 检查最近任务邮箱
- 理解“生成后确认”的执行方式

### 设置页

用于配置运行参数：

- 邮件通道设置
- 代理设置
- OpenAI 全局参数
- 注册参数
- CPA 参数
- 数据库管理
- 已保存服务管理

## 邮件通道说明

### 1. GPTMail

需要配置：

- 服务名称
- API 地址
- API Key
- 可选前缀

当前状态：

- 已完整接入预生成邮箱、验证码轮询和注册执行

### 2. 自定义邮箱 API

需要配置：

- 服务名称
- API 地址
- API Key
- 默认域名/前缀

当前状态：

- 已接入预生成邮箱
- 已接入验证码轮询
- 已接入注册执行

约定接口与原 Python 版兼容，主要包含：

- `GET /api/config`
- `POST /api/emails/generate`
- `GET /api/emails/{emailId}`
- `GET /api/emails/{emailId}/{messageId}`

## 账号管理说明

账号数据保存在本地 SQLite。

当前记录字段包括：

- 邮箱
- 状态
- 密码
- workspace_id
- account_id
- access_token
- refresh_token
- id_token
- session_token
- last_refresh
- expires_at
- cpa_uploaded

### Token 刷新逻辑

优先级：

1. `session_token`
2. `refresh_token`

刷新成功后会更新本地数据库中的：

- `access_token`
- `refresh_token`
- `expires_at`
- `last_refresh`

### Token 验证逻辑

通过调用：

- `https://chatgpt.com/backend-api/me`

按返回状态码判断：

- `200`：有效
- `401`：无效或过期
- `403`：可能封禁

## CPA 功能说明

当前支持：

- 导出 CPA 格式 JSON
- 上传选中账号到 CPA 平台

相关配置项在“设置”页：

- `CPA 启用开关`
- `CPA API URL`
- `CPA API Token`

上传成功后会在本地数据库中标记：

- `cpa_uploaded = true`
- `cpa_uploaded_at = 当前时间`

## 数据库说明

本地数据库文件位置会显示在“设置 -> 数据库管理”中。

当前包含的核心表：

- `email_services`
- `accounts`
- `registration_tasks`
- `app_settings`

当前数据库管理支持：

- 查看数据库路径
- 查看文件大小
- 查看账号数 / 服务数 / 任务数
- 备份数据库
- 清理任务记录

## 实时日志说明

桌面端没有沿用浏览器 WebSocket 方案，而是做成了桌面等价能力：

- Rust 后端任务更新时通过 `Tauri event` 向前端推送 `task://updated`
- 前端订阅事件并实时更新右侧日志区
- 保留轮询作为兜底机制

这意味着：

- 对桌面端来说，日志是实时推送的
- 不需要再依赖浏览器里那套 WebSocket 路由

## 当前限制

当前版本有这些明确限制：

- `Outlook` 通道未迁移
- 没有代理池和动态代理管理
- 没有原 Python 版那种非常细粒度的服务编排能力
- 账号页虽然支持很多操作，但还没有把所有原版按钮完全复刻
- 仍处于高频开发迭代阶段，UI 和字段结构可能继续调整

## 开发建议

如果你继续在这个项目上开发，建议优先顺序：

1. 补齐 `Outlook` 通道
2. 补齐动态代理和代理池
3. 深化账号页 UI
4. 完成更多原 Python 版设置项迁移

## GitHub 上传前说明

当前仓库根目录已经加入 `.gitignore`，并且已经清理掉这些不应提交的目录：

- `node_modules/`
- `dist/`
- `src-tauri/target/`
- `.DS_Store`
- `gptgo-backups/`

保留了锁文件：

- `package-lock.json`
- `src-tauri/Cargo.lock`

因此当前目录结构适合直接提交到 GitHub。

## 免责声明

本项目涉及账号注册、邮件接收、令牌处理和第三方 API 上传。请只在你有权限、且符合目标平台规则的前提下使用。  
任何与平台规则、账号使用条款、代理来源或数据合规有关的责任，需要由实际使用者自行评估和承担。
