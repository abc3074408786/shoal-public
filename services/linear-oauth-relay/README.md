# Linear 扫码授权服务

此服务接收手机或浏览器上的 Linear OAuth 回调，让 Velella 桌面完成连接。它不接收 Linear access token、refresh token 或 client secret。PKCE verifier 只存在于发起请求的桌面进程内。

## 启用前准备

1. 在 Linear 的 API 设置中，以应用维护者身份注册名为 **Velella** 的 OAuth 应用。
2. 选择专用 HTTPS 域名，以下以 `https://auth.example.com` 为示例；不要原样使用示例地址。
3. 将回调地址注册为 `https://auth.example.com/callback`，与域名精确匹配。
4. 记录公开的 Client ID。使用官方 PKCE S256 流程，不需要在桌面或此服务放置 Client Secret。

当前请求 `read,write`，用于任务列表、详情、创建、状态变更与评论。授权前用户可在 Linear 选择工作区并确认权限。

官方契约：<https://linear.app/developers/oauth-2-0-authentication>

## 服务启动（部署操作需另行授权）

Node.js 24 LTS，无第三方 npm 依赖。在服务目录设置 `LINEAR_CLIENT_ID` 和 `LINEAR_PUBLIC_URL` 环境变量，然后运行：

```text
node server.mjs
```

默认仅监听 `127.0.0.1:8787`。由 HTTPS 反向代理转发专用域名的全部路径，支持 GET、POST、DELETE；不要复用 OA 等业务服务的路径。容器构建可使用同目录 Dockerfile，容器端口为 8787。

- `LINEAR_CLIENT_ID`：注册得到的 Client ID。
- `LINEAR_PUBLIC_URL`：HTTPS origin，不带路径、查询参数、用户名或密码。
- `HOST`、`PORT`：可选监听配置，容器中 HOST 为 `0.0.0.0`。
- `GET /health`：只返回服务健康，不代表 Linear OAuth 应用注册正确或真实授权成功。

**代理访问日志必须去掉查询字符串，尤其是 `/callback`。** 不要记录请求体、Authorization header 或响应正文。服务本身只打印启动信息。回调页面不含第三方资源，并禁用缓存、referrer 和嵌入。

当前实现使用内存会话，仅部署单实例。一次授权最长 10 分钟；进程重启后未完成会话失效，需要用户显式重新生成。全局最多 256 个会话，每个直连 IP 每分钟最多创建 20 次；反向代理后共享这一额度，适合当前个人部署。公网边缘应设置请求体与速率限制。

## 桌面配置

在运行 Velella 的数据目录（Windows 默认 `%LOCALAPPDATA%\Velella`）放置 `linear-oauth.json`：

```json
{
  "relay_url": "https://auth.example.com",
  "client_id": "注册得到的 Client ID"
}
```

该配置只有服务地址和公开应用标识，不含密钥。每次生成二维码时重新读取配置，无需为配置文件单独重启运行时，但必须安装含本次代码的桌面和 runtime 才会出现新入口。

## 验收与回退

1. 使用新版本候选和隔离数据目录，确认未配置时明确显示“扫码授权尚未启用”。
2. 配置真实服务，生成二维码；手机扫码或本机打开同一链接，在 Linear 完成授权。
3. 电脑自动显示连接的工作区，读取团队与任务；真实账号验证不能用 fixture 测试替代。
4. 活动授权关闭弹窗前等待后端取消回执；回执确认取消后不再保存迟到的授权。若授权已经先完成，则显示已经保存的连接。强制退出桌面或网络中断时结果可能未知，下次打开来源列表核对。拒绝和过期后可显式重新生成。
5. access/refresh token 以现有系统凭据保护保存于来源数据库，不经 UI 或 Agent。Token 即将过期时自动刷新；刷新失败要求重新连接，不会无限重试。

撤下服务会停止新扫码连接，已保存连接仍可直接向 Linear 刷新。Velella 的“断开来源”会移除本地凭据并保留任务关联；若要撤销 Linear 平台授权，还需在 Linear 中撤销该应用。

测试：`node --test services/linear-oauth-relay/server.test.mjs`（仓库根目录）。不要把测试 client、fixture 域名或 fixture QR 用于真实登录。
