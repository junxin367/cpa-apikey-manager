# CPA API 密钥权限与额度管理

[![构建与发布](https://github.com/junxin367/cpa-apikey-manager/actions/workflows/build-release.yml/badge.svg)](https://github.com/junxin367/cpa-apikey-manager/actions/workflows/build-release.yml)

CLIProxyAPI 原生插件：读取宿主已创建的 API 密钥，为每把密钥设置模型允许／禁止规则，以及按 Token 或金额计算的周期额度。自带独立中文管理页面。

[下载发布产物](https://github.com/junxin367/cpa-apikey-manager/releases/latest) · [查看自动构建](https://github.com/junxin367/cpa-apikey-manager/actions)

## 功能

- 只读同步 `access.api-keys`，兼容旧配置根级 `api-keys` 字符串列表；不会修改或创建宿主密钥。
- 只管理显式配置的模型；未配置模型直接放行，不创建额度请求或用量记录。
- 支持 GPT、Claude、DeepSeek、GLM、Kimi 等模型分组，模型 ID 可从宿主同步或手动添加。
- 校验宿主解析后的实际模型，多个已知别名共用目标模型的额度。
- 每个模型分别选择 Token／USD 额度与上限；每把密钥独立选择一个日／周／月周期，供其所有模型共用。
- 使用宿主服务器的系统时区：每天 00:00、每周一 00:00、每月 1 日 00:00 重置。
- 以自然周期汇总账本，无需午夜清空数据库；跨零点请求按准入开始时间归属，重启不会丢失已记账用量。
- 自动从 models.dev、LiteLLM、OpenRouter 补全缺失价格；支持手动设置，已有价格（包括 0）不会被覆盖。
- 金额按输入、输出和缓存单价计算，使用定点数保存，精确到 `0.000001 USD`。
- 配置切换保留用量；没有历史价格时先显示补计费用，再由管理员确认保存。
- 支持流式和非流式用量、重复通知去重、失败请求的实际消耗，以及缺失用量的人工核对。
- 拒绝请求时返回中文原因和处理建议；额度耗尽时列出模型、周期、已用量、上限及重置时间。
- 管理接口复用宿主管理鉴权；浏览器内存保存管理密钥，不写入浏览器持久化存储。

## 已验证环境与边界

- 在 Windows x64、CLIProxyAPI **v8.0.4** 官方二进制中实际加载 DLL，验证了 Chat Completions、Responses、Anthropic Messages，以及 SSE 流式请求。
- 验证使用本地模拟模型服务，没有调用收费模型。完整结果见 [验证记录](docs/verification.md)。
- 自动构建覆盖 Windows x64、Linux x64、macOS Intel 和 Apple Silicon；Linux／macOS 尚未进行宿主功能验收。
- 不承诺兼容没有原生插件能力的旧宿主；支持旧密钥字段格式不等于支持所有旧版二进制。
- 第一版用于单个宿主进程，不提供多宿主共享额度。请勿让不同宿主共用同一数据库。
- **额度按实际结算用量限制后续请求，已经放行的并发或流式请求可能超额。**这不是严格预付费或消费绝不超限的计费系统。
- 插件必须保持启用。宿主禁用、卸载或熔断插件后，插件无法继续拦截请求。
- 本版面向文本 Token 计费；未验证 Responses WebSocket，不提供图像、音频、视频等单独计价。
- 插件安装前的历史用量不自动补造。页面显示开始记录时间。

## 安装

从 [GitHub Releases](https://github.com/junxin367/cpa-apikey-manager/releases/latest) 下载对应平台的 ZIP，解压后安装。ZIP 内动态库已使用正确的插件文件名；独立下载的带版本动态库需要手动重命名。使用 Release 的 `checksums.txt` 校验下载文件，ZIP 内 `SHA256SUMS.txt` 校验安装用动态库。

1. 将对应平台的动态库放到宿主 `plugins` 目录，文件名必须是：

   | 平台 | 安装文件名 |
   | --- | --- |
   | Windows x64 | `cpa-apikey-manager.dll` |
   | Linux | `cpa-apikey-manager.so` |
   | macOS | `cpa-apikey-manager.dylib` |

2. 将本项目 `config.example.yaml` 中的 `plugins` 配置合并到现有宿主配置，填写：
   - `cpa-config-path`：宿主实际使用的配置文件绝对路径。
   - `cpa-base-url`：该宿主的 HTTP(S) 地址，用于读取模型目录。
   - `data-dir`：可写的数据目录，推荐绝对路径。

3. 宿主需要启用管理 API 并设置管理密钥。v8 配置位于 `management.secret-key`；客户端 `access.api-keys` 不能代替管理密钥。
4. 启动或重启宿主。在日志确认插件加载成功，并确认不存在 `invalid metadata`、注册失败或熔断错误。
5. 打开下面的页面，将端口改成你的宿主端口：

   ```text
   http://127.0.0.1:8317/v0/resource/plugins/cpa-apikey-manager/index.html
   ```

6. 输入宿主管理密钥，选择客户端密钥。填写备注与额度周期后点击顶部“保存配置”；同步模型后，可逐行设置权限，或点击“设置额度／编辑额度”打开集中编辑弹窗。

页面及静态资源嵌入动态库，无需 Node.js、前端构建或独立 Web 服务器。公开页面资源不含密钥和用量数据；实际数据接口必须通过宿主管理鉴权。通过反向代理部署时，需要同时转发插件资源路径和管理 API 路径。

新同步密钥的模型不受插件限制。仅显式禁止的模型返回 403；显式设置额度后才开始记账并检查上限。本版不提供“未单独配置的模型”默认权限开关。

## 管理页面

导航仅包含 **API 密钥** 和 **模型价格**，没有设置页或用量明细页。

- 密钥页按模型展示权限、额度摘要、当前用量和重置时间。额度弹窗集中选择“不限额／Token／金额 USD”，填写上限后点击“保存额度”直接生效。Token 与金额分别保留本次编辑的输入值，切换单位不会把 Token 数直接当作金额。
- 备注、密钥周期和行内权限通过顶部“保存配置”提交。如果打开额度弹窗时已有其他未保存的密钥修改，弹窗会明确提示本次保存将一并提交。
- 价格页支持名称与价格状态筛选，展示价格来源、同步日期及后台同步状态。
- 页面参考 CPA-Manager-Plus 的浅色管理台风格；使用统一设计变量、14px 正文、32px 紧凑控件和最大 1440px 内容区。权限选择框宽 88px，额度输入框宽 224px；窄屏通过重排与表格内部横向滚动适配。
- 待核对用量通过对应模型行的“处理待核对”入口处理，不提供独立明细页面。

![密钥管理](docs/images/management.jpg)

![集中编辑额度](docs/images/quota-editor.jpg)

## 额度与价格规则

### 周期

| 周期 | 新额度开始时间 |
| --- | --- |
| 每天 | 当前时区次日 00:00 |
| 每周 | 当前时区下周一 00:00 |
| 每月 | 当前时区下月 1 日 00:00 |

周期在每把密钥的详情顶部设置。时区只读展示，来自宿主服务器的系统时区；旧插件配置或数据库中的 `timezone` 不再参与计算。修改服务器系统时区后建议重启宿主，使运行环境保持一致。周期改变时重新汇总已有记录，不删除历史用量。

“不限额”和“额度为 0”不同：不限额不检查上限；0 会立即拒绝该模型的新请求。被禁止的模型始终返回 403，不会因存在额度而开放。

额度耗尽返回 429、`quota_exceeded`、`reset_at` 和 `Retry-After`。降低额度至已用量以下立即阻止新请求；提高额度后可恢复调用。

### 金额

价格单位为 **USD / 100 万 Token**。输入价和输出价必填；缓存读取／写入价留空时沿用输入价。推理 Token 已包含在对应输出部分，不重复计价。

```text
费用 =（普通输入 × 输入价 + 输出 × 输出价
      + 缓存读取 × 缓存读取价 + 缓存写入 × 缓存写入价）/ 1,000,000
```

每次执行总费用向上取整到 `0.000001 USD`。公开目录价格仅作为参考，不代表上游实际账单，也不读取上游账户余额。

修改价格对之后放行的请求生效；已放行请求保留价格快照。切换到金额额度时，当前周期的未计价记录必须先按当前单价补计并确认；若仍有未计价的执行中请求，需等待其结算。

### 自动补全价格

启动、发现新模型以及默认每 6 小时检查一次缺失价格，也可以在价格页点击“同步缺失价格”。固定来源顺序如下：

```text
https://models.dev/catalog.json
https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json
https://openrouter.ai/api/v1/models
```

只接受明确且唯一的模型匹配；models.dev 优先采用官方规范模型身份，不用模糊名称匹配猜价格。存在歧义、缺少输入／输出价、来源不可用时保持未设置，页面说明结果。LiteLLM 和 OpenRouter 的每 Token 价格会转换为每百万 Token 价格，科学记数法采用十进制定点解析。

自动同步只补全从未设置的价格，不更新已有自动价格，也不会覆盖管理员设置的价格或零价格。后台任务使用独立数据库连接；并发手动保存优先，不阻塞模型请求等待外网。同步只匿名下载公开目录，不发送客户端密钥、管理密钥或用量数据。

导入范围仅为基础文本输入、输出、缓存读写四种费率，不包含阶梯、优先级、长上下文和图像／音频／视频等特殊计费。缓存费率缺失时沿用输入价，明确的零价格会保留。自动补价不追溯修改已结算费用。

`price-sync` 的开关、1～168 小时检查间隔和镜像 URL 在宿主插件配置中设置，见 `config.example.yaml`。全部来源失败时保留有效价格，并在 5 分钟后重试。

### 拒绝请求的中文错误

插件统一以 `application/json; charset=utf-8` 返回错误。`error.message` 是供用户阅读的中文说明，`error.code` 保留稳定的机器错误码，便于调用程序判断原因。例如：

```json
{
  "error": {
    "code": "model_forbidden",
    "message": "请求被拒绝：当前 API 密钥无权调用模型「gpt-local」。请联系管理员开通该模型权限。"
  }
}
```

| HTTP 状态 | 错误码 | 中文说明包含的信息 |
| --- | --- | --- |
| 403 | `model_forbidden` | 被禁止的实际模型、联系管理员开通权限 |
| 403 | `unknown_api_key` | 密钥无法识别或已移除、检查密钥或联系管理员 |
| 429 | `quota_exceeded` | 模型、日／周／月周期、已用量、额度上限、重置时间和时区 |
| 503 | `usage_needs_review` | 待核对模型和请求数量、管理页核对入口 |
| 503 | `unpriced_usage` | 模型缺少价格或用量尚未计价、设置价格或确认补计费用 |
| 503 | `accounting_unavailable` / `storage_unavailable` | 记账或数据库异常、管理员检查和恢复办法 |
| 503 | `key_source_unavailable` / `plugin_unavailable` | 宿主配置或插件异常、检查相应配置 |
| 503 | `request_context_unavailable` | 宿主缺少请求标识、检查版本兼容性 |
| 400 | `missing_model` | 请求缺少模型、填写 `model` 字段 |

额度错误同时提供 `error.model`、`unit`、`period`、`used`、`limit`、`reset_at` 和 `timezone` 字段，以及 `Retry-After` 响应头。`reset_at` 是 UTC 的 RFC 3339 时间；中文消息中的重置时间按服务器系统时区显示。Token 数和金额均用字符串返回，避免大数精度丢失。

流式请求在准入时被拒绝，也会收到上述 JSON 错误。宿主在进入插件前产生的鉴权错误及上游服务错误由对应服务决定。

### 用量异常

完成通知先到不代表用量为 0。完成后超过 30 秒仍无用量，或插件重启发现遗留未结算请求，会将其标记为“待核对”。待核对状态在访问管理页或处理新请求时检查。

对应密钥和模型的新请求暂时返回 503。管理员在“API 密钥 → 对应模型 → 处理待核对”中填写 Token 明细及核对原因；迟到的真实用量会替换人工记录，避免重复扣减。未知／不一致的 Token 计量不会悄悄按零费用处理。

数据库写入失败时会阻止后续调用，修复数据目录或磁盘问题后重启插件。宿主配置来源不可读时同样返回 503，读取恢复后自动同步。

### 旧数据迁移

旧规则中的逐模型周期统一迁移为密钥级周期：原周期相同时保留，存在不同周期时采用每月，没有额度规则时采用每天。管理员可再为每把密钥调整周期。旧 `default_access` 被移除，未配置模型直接放行；旧请求和用量记录保留。

## 从源码构建

通过 rustup 使用 `rust-toolchain.toml` 固定的 Rust 1.95.0 工具链。Windows 使用 MSVC 工具链和 Visual Studio C++ Build Tools；Linux／macOS 需要 C 编译器（SQLite 随包构建）。打包脚本需要 PowerShell 7。

```powershell
pwsh -File scripts/build.ps1
```

脚本生成 `dist/cpa-apikey-manager-windows-x64.zip`，其中包含动态库、配置示例、本文档、MIT 许可证、验证记录、页面截图和 SHA-256 校验文件。实际平台名称随运行系统变化。

加上 `-ReleaseAssets` 会生成包含版本和平台的 ZIP 及独立动态库，例如 `cpa-apikey-manager-0.1.0-windows-x64.zip` 和 `cpa-apikey-manager-0.1.0-windows-x64.dll`。

也可以直接构建：

```text
cargo build --release --locked
```

将 `target/release/` 中的 `cpa_apikey_manager.dll`、`libcpa_apikey_manager.so` 或 `libcpa_apikey_manager.dylib` 重命名为前述安装文件名。不要把 Rust 自动生成的下划线文件名直接作为插件 ID 使用。

插件元信息默认使用 `Cargo.toml` 中的本仓库地址。GitHub Actions 会通过 `CPA_PLUGIN_REPOSITORY` 写入实际构建仓库地址，派生仓库无需修改插件源码。

## GitHub 自动构建与发布

发布方式参考 [cpa-window-primer](https://github.com/junxin367/cpa-window-primer)，使用一个工作流完成检查、四平台构建和产物发布：

| 触发方式 | 行为 |
| --- | --- |
| 推送 `main`、提交 Pull Request | 检查 Rust 格式、JavaScript 语法，执行 `cargo test --locked`，构建四个平台并保存 Actions 产物 |
| 推送 `v*`／`V*` 版本标签 | 校验标签与 Cargo 版本一致，完成全部构建后自动创建或补全 GitHub Release |
| 发布已有标签的 GitHub Release | 为该标签补全产物，保留已有发布说明 |
| Actions → 构建与发布 → Run workflow | 标签留空仅构建；填写已有标签时构建并补全该版本 Release |

每个平台上传一个 ZIP 和一个独立动态库，另提供汇总 `checksums.txt`、中文发布说明和与参考项目格式一致的 `latest.json`。仅在所有平台构建成功后发布完整产物；编译阶段只读仓库，发布阶段才允许写入 Release。Action 固定到提交 SHA，Rust 版本固定在 `rust-toolchain.toml`，依赖使用 `Cargo.lock`。

Actions 临时产物保留 14 天；版本产物保存在 GitHub Releases。Linux 使用 Ubuntu 22.04 构建，macOS 分别使用 Intel 与 Apple Silicon 原生运行器。

发布新版本时先更新 `Cargo.toml` 的 `version`，运行 Cargo 同步 `Cargo.lock` 并提交，再推送同版本标签，例如：

```text
git tag v0.1.1
git push origin main v0.1.1
```

标签必须指向包含工作流的提交，且版本与该提交的 `Cargo.toml` 一致。带预发布后缀的标签（如 `v0.2.0-rc.1`）会创建 GitHub 预发布版本。

## 备份与卸载

先停止宿主再备份整个 `data-dir`。SQLite 数据库同时包含用量、配置、价格和密钥 HMAC 所需的随机密钥；客户端明文密钥仅从宿主配置读入内存，不写入插件账本。

卸载前明确设置 `plugins.configs.cpa-apikey-manager.enabled: false`，停止宿主后移除动态库。数据库可以保留，以便以后恢复；插件停用期间不会记录或限制用量。

## 源码结构

```text
src/native.rs      C ABI、生命周期与 RPC 分发
src/config.rs      宿主密钥和模型别名同步
src/accounting.rs  自然周期、定点金额、Token 归一化
src/engine.rs      权限、额度、用量结算和持久化
src/price_catalog.rs  公开目录解析、模型匹配和精确费率转换
src/price_sync.rs     后台自动补价、来源回退与并发保护
src/schema.sql     SQLite 表及索引
src/web.rs         管理接口与页面资源
ui/               独立中文管理页
scripts/build.ps1  构建与打包
scripts/release.mjs  版本校验、发布说明、校验清单与 latest.json
.github/workflows/build-release.yml  四平台自动构建与发布
```

具体验证范围与结果见 `docs/verification.md`。
