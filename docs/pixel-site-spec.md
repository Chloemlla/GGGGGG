# Pixel 自助兑换 Gemini 会员系统复刻规格

来源：基于 `https://pixel.yh-mo.xyz/` 公开 HTML、打包脚本、页面文本和可访问接口响应整理。本文档用于复刻页面功能和 API 契约，不包含原站私有实现、真实卡密数据、第三方文档内容或受保护素材。

## 1. 页面总览

站点是一个 Vite 打包的单页应用，原前端使用 Vue。复刻项目要求改用 React 19，后端使用 Rust。公开路由如下：

| 路由 | 功能 |
| --- | --- |
| `/` | 主操作页：卡密验证、服务类型选择、账号提交、历史任务、任务详情、导出、取消排队 |
| `/api-doc` | API 文档页：验证卡密、提交任务、查询任务、任务列表 |

页面标题：`自助兑换 Gemini 会员系统`。

## 2. 主页面功能

### 2.1 操作说明

主页面顶部提供操作说明和两个入口：

- `API文档`：跳转到 `/api-doc`。
- `查看完整文档`：打开飞书文档外链。

公开页面列出的流程：

1. 输入卡密并验证，确认剩余额度。
2. 在账号列表中填写账号信息，格式：`邮箱----密码----辅助邮箱----2FA密钥`。
3. 提交任务后等待系统自动处理，完成后返回优惠链接。
4. 任务详情里可以取消仍在排队的账号，取消后自动回补未消费额度。

### 2.2 温馨提示弹窗

页面有一个本地弹窗：

- 标题：`温馨提示`。
- 内容提示：如果需要用 `提取链接 + 绑卡`，请先确保账号没有支付资料，否则会不成功；已有支付资料请先删除支付资料再进行提取链接 + 绑卡。
- 支持 `今天不再弹出`。
- localStorage key：`payment_warning_dismissed_date`。
- 日期格式：`YYYY-MM-DD`。

### 2.3 卡密验证

用户输入卡密后点击 `验证` 或按 Enter：

- 请求 `POST /api/verify-card`。
- 成功时显示 `剩余额度` 和 `总额度`。
- 若接口返回 `valid: false` 但仍带有额度字段，页面显示信息提示，并允许查看该卡密的历史任务。
- 验证成功后自动查询历史任务。

页面兼容两组额度字段：

- 新字段：`remaining_quota`、`total_quota`、`remaining_quota_units`、`total_quota_units`。
- 旧字段：`remaining`、`total_count`。

### 2.4 服务类型

服务类型由前端固定配置，同时受 `/api/settings` 开关控制。

| value | 页面标签 | 预计消耗 | 开关 | 关闭提示 | 备注 |
| --- | --- | ---: | --- | --- | --- |
| `link_only` | 只提取链接 | 0.5 额度/账号 | 无 | 无 | 默认可用 |
| `link_and_bind` | 提取链接+手机绑卡 | 1 额度/账号 | `auto_bind_enabled` | 目前手机绑卡暂时关闭 | 已有优惠链接时仅扣 0.5 额度 |
| `link_and_bind_1usd` | 提取链接+一刀卡绑卡 | 1.5 额度/账号 | `auto_bind_one_dollar_enabled` | 目前一刀卡绑卡暂时关闭 | 已有优惠链接时仅扣 1 额度 |

如果当前选中的服务被设置接口关闭，前端会切回 `link_only`。

### 2.5 账号提交

账号列表为多行文本，每行一个账号：

```text
邮箱----密码----辅助邮箱----2FA密钥
```

页面行为：

- 统计非空行作为账号数。
- 根据服务类型显示预计最多消耗额度。
- 未输入卡密时提示 `请先输入并验证卡密`。
- 未输入账号时提示 `请输入账号信息`。
- 提交时请求 `POST /api/submit-task`。
- 提交成功后：
  - 显示接口返回的 `message`。
  - 记录 `task_id`。
  - 开始轮询任务详情。
  - 刷新卡密额度和历史任务列表。

### 2.6 任务轮询

任务提交或点击历史任务 `查看` 后：

- 请求 `GET /api/task/{task_id}`。
- 每 5 秒轮询一次。
- 当任务状态为 `completed`、`failed` 或 `cancelled` 时停止轮询。
- 任务详情弹窗展示任务状态、进度条、已取消数量和账号明细。

账号明细表字段：

| 字段 | 页面含义 |
| --- | --- |
| `id` | 账号记录 ID，取消排队时使用 |
| `line_number` | 原始账号行号 |
| `email` | 账号邮箱 |
| `status` | 账号状态 |
| `message` | 处理信息 |
| `result_link` | 优惠链接，成功时可点击 |
| `queue_position` | 排队位置，仅 `pending` 时显示 |

### 2.7 状态枚举

任务状态：

| status | 页面文案 |
| --- | --- |
| `pending` | 排队中 |
| `running` | 运行中 |
| `completed` | 已完成 |
| `failed` | 失败 |
| `cancelled` | 已取消 |

账号状态：

| status | 页面文案 |
| --- | --- |
| `pending` | 排队中 |
| `running` | 运行中 |
| `success` | 成功 |
| `failed` | 失败 |
| `bind_pending` | 待绑卡 |
| `binding` | 绑卡中 |
| `bind_success` | 绑卡成功 |
| `bind_failed` | 绑卡失败 |
| `cancelled` | 已取消 |

页面统计规则：

- 成功：`success`、`bind_success`。
- 失败：`failed`、`bind_failed`。
- 已完成进度：`success`、`failed`、`bind_success`、`bind_failed`、`cancelled`。
- 导出成功账号：`success`、`bind_pending`、`binding`、`bind_success`、`bind_failed` 中有 `result_link` 的记录。

### 2.8 历史任务

历史任务基于 `卡密 + 账号关键字` 查询：

- 请求 `POST /api/tasks-by-card`。
- 搜索框 placeholder：`输入账号关键字，支持模糊搜索`。
- 支持 `查询账号`、`重置`。
- 每页 10 条。
- 分页按钮：`首页`、`上一页`、`下一页`、`末页`。

历史任务表字段：

| 字段 | 页面含义 |
| --- | --- |
| `task_id` | 页面显示前 8 位加省略号 |
| `total_accounts` | 账号数 |
| `success` | 成功数量 |
| `failed` | 失败数量 |
| `status` | 任务状态 |
| `created_at` | 创建时间 |
| 操作 | `查看`，有成功账号时显示 `导出` |

空状态：

- 无历史任务：`当前卡密还没有历史任务`。
- 搜索无匹配：`当前卡密下没有匹配该账号的历史任务`。
- 加载中：`历史任务查询中...`。

### 2.9 导出

单任务导出：

- 先请求 `GET /api/task/{task_id}`。
- 筛选有 `result_link` 的成功/绑卡相关账号。
- 下载文件名：`task_{task_id前8位}_success.txt`。
- 文件内容每行：`email----result_link`。

全部导出：

- 请求 `POST /api/tasks/export-by-card`。
- 请求体包含 `card_key` 和可选 `account_query`。
- 下载文件名：`all_tasks_success_YYYYMMDD_HHMMSS.txt`。
- 文件内容每行：`email----result_link`。

### 2.10 取消排队

任务详情中，只有账号状态为 `pending` 时显示 `取消排队`。

- 点击前弹出确认：`确认取消 {邮箱或第N行} 的排队吗？取消后会回补未消费额度。`
- 请求 `POST /api/task/{task_id}/account/{account_id}/cancel-queue`。
- 成功后刷新卡密额度和任务详情。

## 3. API 规范

Base URL：站点同源，例如 `https://pixel.yh-mo.xyz`。

通用请求头：

```http
Content-Type: application/json
```

通用错误格式：

```json
{
  "detail": "错误信息"
}
```

已观察到的错误行为：

- `GET /api/task/uuid-task-id` 不存在时返回 `404 {"detail":"任务不存在"}`。
- `POST /api/submit-task` 使用无效卡密返回 `400 {"detail":"卡密不存在"}`。
- `POST /api/tasks-by-card` 使用无效卡密返回 `400 {"detail":"卡密不存在"}`。
- 对只支持 POST 的接口使用 GET 返回 `405 {"detail":"Method Not Allowed"}`。

### 3.1 获取设置

页面实际调用，但公开 API 页未列出。

```http
GET /api/settings
```

响应示例：

```json
{
  "auto_bind_enabled": true,
  "auto_bind_one_dollar_enabled": false
}
```

### 3.2 验证卡密

公开 API 页列出，限制：20 次/分钟。

```http
POST /api/verify-card
```

请求体：

```json
{
  "card_key": "your-card-key"
}
```

成功响应：

```json
{
  "valid": true,
  "remaining": 10,
  "total_count": 20,
  "remaining_quota": "5",
  "total_quota": "10",
  "remaining_quota_units": 10,
  "total_quota_units": 20,
  "message": "卡密有效"
}
```

无效卡密响应仍为 200：

```json
{
  "valid": false,
  "remaining": null,
  "total_count": null,
  "remaining_quota": null,
  "total_quota": null,
  "remaining_quota_units": null,
  "total_quota_units": null,
  "message": "卡密不存在"
}
```

### 3.3 提交任务

公开 API 页列出，限制：10 次/分钟。

```http
POST /api/submit-task
```

请求体：

```json
{
  "card_key": "your-card-key",
  "service_type": "link_only",
  "accounts_text": "email1----password1----recovery1----2fakey1\nemail2----password2----recovery2----2fakey2"
}
```

`service_type` 可选值：

- `link_only`
- `link_and_bind`
- `link_and_bind_1usd`

响应示例：

```json
{
  "task_id": "uuid-task-id",
  "total_accounts": 2,
  "message": "任务提交成功"
}
```

### 3.4 查询任务状态

公开 API 页列出，限制：60 次/分钟。

```http
GET /api/task/{task_id}
```

响应示例：

```json
{
  "task_id": "uuid-task-id",
  "status": "running",
  "total_accounts": 2,
  "accounts": [
    {
      "id": 1,
      "line_number": 1,
      "email": "test@example.com",
      "status": "success",
      "message": "处理成功",
      "result_link": "https://example.com/link"
    }
  ]
}
```

页面还会读取 `queue_position`。

### 3.5 查询卡密所有任务

公开 API 页列出，限制：20 次/分钟。

```http
POST /api/tasks-by-card
```

请求体：

```json
{
  "card_key": "your-card-key",
  "account_query": "optional-email-keyword"
}
```

`account_query` 是页面实际传入字段；公开 API 页示例中省略该字段。

响应示例：

```json
{
  "tasks": [
    {
      "task_id": "uuid-task-id",
      "total_accounts": 2,
      "status": "completed",
      "success": 2,
      "failed": 0,
      "created_at": "2026-03-28 10:00:00"
    }
  ],
  "message": "查询成功"
}
```

页面可兼容额外字段：`cancelled`、`done`。

### 3.6 导出卡密成功账号

页面实际调用，但公开 API 页未列出。

```http
POST /api/tasks/export-by-card
```

请求体：

```json
{
  "card_key": "your-card-key",
  "account_query": "optional-email-keyword"
}
```

响应体：

```json
{
  "accounts": [
    {
      "email": "test@example.com",
      "result_link": "https://example.com/link"
    }
  ],
  "message": "查询成功"
}
```

### 3.7 取消排队账号

页面实际调用，但公开 API 页未列出。

```http
POST /api/task/{task_id}/account/{account_id}/cancel-queue
```

响应体：

```json
{
  "message": "已取消"
}
```

## 4. 复刻实现建议

### 4.1 前端

- React 19 + Vite。
- `/` 和 `/api-doc` 两个 SPA 路由。
- 使用同源 `/api` 调用后端，开发环境通过 Vite proxy 转发到 Rust API。
- 保留前端轮询间隔 5 秒。
- 保留 localStorage 的 `payment_warning_dismissed_date` 行为。
- 导出使用浏览器 Blob 下载纯文本。

### 4.2 后端

- Rust Axum 提供 JSON API。
- 真实业务中需要持久化以下实体：
  - 卡密：额度、总额度、状态。
  - 任务：任务 ID、卡密、服务类型、创建时间、总体状态。
  - 账号记录：行号、邮箱、状态、消息、结果链接、排队位置。
  - 额度流水：提交扣减、取消回补、失败回补策略。
- 复刻 mock 可以先使用内存数据，保留同样响应结构。

### 4.3 GitHub Actions

工作流需要在 push、PR 和手动触发时：

1. 安装 Node。
2. 构建 `frontend/dist`。
3. 安装 Rust。
4. 构建后端 release 二进制。
5. 使用 `actions/upload-artifact` 上传前后端构建产物。
