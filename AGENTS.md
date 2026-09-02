# AGENTS.md — 「月兔娘」Kimi 额度桌面挂件

> 本文件面向 AI 编码代理。阅读前请假设自己对本项目一无所知。

## 项目现状（重要）

**本项目已完成首版实现（2026-09-02），技术选型已经用户确认：Tauri v2
（Rust + 系统 WebView2）+ 原生 JS 前端，数据路线为「记账模式为主线 +
CLI /usage 解析增强，自动降级」。** 不要再重新发起选型讨论。

仓库结构：

- `需求规格书.md` —— 原始需求规格书（原 readme.md，因 Windows 文件名大小写不敏感与交付 README 冲突而改名）（新需求以它为准）
- `README.md` —— 交付用中文说明文档
- `src/` —— 挂件前端（`index.html` + `widget.js`，原生 JS 无构建步骤，
  由 Tauri 直接静态托管；`src/assets/rabbit.png` 为月兔娘素材）
- `src-tauri/` —— Rust 后端：`src/sources/` 是数据源可插拔适配层
  （`ledger.rs` 记账主线 / `cli_usage.rs` pty 抓 /usage / `mod.rs`
  降级编排），`src/config.rs` 配置读写，`src/bin/probe_usage.rs`
  是 /usage 格式探针
- `Kimi月兔娘Q版.png` —— 原始素材（注意：实为 1024×1024 无透明通道
  整幅插画，前端按圆形「月亮徽章」裁切展示）
- `docs/screenshot.png` —— README 效果图

关键事实（调研实测结论，勿重新假设）：

- Kimi Code 没有公开额度 API；`kimi web` 本地 REST（56 个端点）无额度
  接口；`/usage` 是 TUI 专属命令
- 记账数据源：`~/.kimi-code/sessions/*/session_*/agents/*/wire.jsonl`
  中的 `{"type":"usage.record",...,"time":<epoch ms>}` 行
- /usage 面板格式见 `cli_usage.rs` 解析器注释（周额度/5h 窗口百分比 +
  `resets in` 倒计时）

## 项目概述

目标：仿照开源项目 [DeepSeek-Balance-Whale-Widget](https://github.com/MeteorNOX/DeepSeek-Balance-Whale-Widget)
（MIT 协议）做一个「月兔娘」桌面悬浮窗挂件，常驻屏幕右下角，监控用户的
Kimi Coding Plan 额度。

背景知识（需求规格书.md 明确给出，不要重新假设）：

- Kimi Coding Plan 是订阅制：额度每 7 天自动刷新，未用完不累积
- 另有每 5 小时的滚动频率窗口，短时间请求过多会限流
- 官方查询途径只有：Kimi Code CLI 内的 `/usage`、Kimi Code 控制台网页
- 订阅额度用尽后有「加油包」（人民币余额）兜底
- **Kimi Code 没有公开的余额查询 API**。不要假设存在
  `api.moonshot.cn/v1/users/me/balance` 之外的接口；开放平台的按量计费
  余额与 Coding Plan 额度是两套体系，不可混用

## 既定工作流程约束

需求规格书.md 规定了四步流程，代理必须遵守：

1. **调研**：读参考仓库（README.md、lib/index.js、whale-widget-prompt.md），
   梳理功能清单；调研 Kimi 额度数据获取途径
2. **先交方案，不写代码**：输出功能映射表、三条数据获取路线（CLI /usage
   解析、本地记账模式、控制台网页令牌模式）的可行性对比与主备推荐、
   桌面悬浮窗技术选型（Tauri / Electron / PyQt 等）。**用户确认方案后才
   允许开始写代码**
3. **实现**：按下方功能与工程要求
4. **交付 GitHub**：git init、写 .gitignore、用 `gh` 创建公开仓库
   `kimi-rabbit-widget`（gh 不可用则停下来等用户手动建仓）、中文
   README、推送 main 分支

## 功能要求摘要（实现阶段）

显示内容：

- 本周额度已用/剩余（进度条 + 百分比）+ 下次刷新倒计时
- 5 小时滚动窗口状态
- 加油包余额（数据源不可得时菜单里隐藏该项）
- 每轮对话消耗估算（可开关）

交互与动效（对齐原项目）：

- 右下角常驻，60 秒自动刷新 + 点击兔娘手动刷新
- 数字变化滚动动画；拖拽 + 四边四分之一吸附；左吸附时整体水平镜像翻转
- 按压 Q 弹效果（按压时底部坐标不变）
- 汉堡菜单：大小滑块、音效开关与音量、气泡开关、数据源切换
- 随机台词气泡：加权随机，5 秒自动收起

形象与文案：

- 「月兔娘」：住月球暗面的兔耳女孩，银白发蓝紫渐变，透明背景 cut-out PNG
- 配色：暗夜蓝紫 + 月光银（**不要深海蓝**）
- 台词口吻：安静、爱读书、熬夜陪用户写代码，额度快没时会着急

## 工程要求

- 数据源做成**可插拔适配层**；CLI 解析失败时自动降级到记账模式，
  不报错崩溃
- 配置文件本地存储；**令牌等敏感信息不明文落盘**
- 任何 API Key、登录令牌、个人额度数据**不允许提交进 Git**；
  .gitignore 必须排除 token、配置、构建产物
- 成品需保留参考项目原作者的署名与 MIT 许可说明（README 中致谢）
- README 用中文撰写，包含：效果图占位、功能列表、安装步骤、
  数据获取原理、各模式配置方法、致谢与许可

## 构建与测试命令

前提：Node 18+、Rust stable（`~/.cargo/bin`）、Tauri 系统依赖（Windows 自带 WebView2）。

```bash
npm install                                # 安装 @tauri-apps/cli（仅此一个依赖）
export PATH="$HOME/.cargo/bin:$PATH"       # Git Bash 下让 cargo 可用
npm run dev                                # 开发模式（GUI 窗口，占用终端）
npm run build                              # 发布构建，产物在 src-tauri/target/release/bundle/
cargo test --manifest-path src-tauri/Cargo.toml      # 后端单元测试（18 个）
cargo run --manifest-path src-tauri/Cargo.toml --bin probe_usage  # 抓一次真实 /usage 输出（生成 probe_usage_output.txt，勿提交）
node --check src/widget.js                 # 前端语法检查
```

注意：Git Bash 每个新会话都要重新 export PATH；前端可脱离 Tauri 直接用
浏览器打开 `src/index.html` 预览（自动进入 mock 数据模式）。

## 语言与文档约定

- 项目文档与 README 使用**中文**；与用户沟通使用中文
- 代码风格、目录结构约定：尚无，待工程初始化后随技术选型补充到本文件
