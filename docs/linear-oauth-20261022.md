# Linear 本机浏览器授权（当前交付）

用户最终选择：本机浏览器一键授权，不使用公网服务，不涉及 OA。以下当前结果取代后文的历史扫码方案。

- 已实现并安装：点击授权后自动打开一次浏览器，提供重开入口；本机临时回调 `http://127.0.0.1:17887/callback`，PKCE S256，无 Client Secret，令牌使用现有 Windows DPAPI 保护。
- 用户已在本次操作中明确确认 zpsine 工作区读写权限。真实授权成功，安装版保存了 OAuth 来源；团队 `ZPS · Zpsine`、首屏 50 条未完成任务及 `ZPS-73` 详情读取通过，后续页存在。
- Linear 应用已移除旧公网回调。桌面配置只有公开 Client ID；本次未部署公网服务、隧道或 DNS，OA 未参与。
- 开发验证：集成 Rust 187 通过、3 忽略；前端构建通过；浏览器 10 项通过；隔离桌面候选 4 项通过。独立静态复核发现取消后立即重试时端口释放存在窗口，已改为同步释放；修复后 12 项 Linear 定向测试通过。未另行运行独立测试，不宣称独立验收通过。
- 安装使用保留 safe-recovery 的集成源和独立构建目录。仅替换 app.asar/runtime，升级检查保留 16 张表、30 个文件前缀、10 个 GitHub 来源和 18 条会话。GUI 主程序不变，已有安装备份。
- 当前真实账号连接与读取已完成；正式安装后的原生界面交互、真实业务写入和长期刷新 UAT 尚未执行。GitLab、Jira 真实账号连接仍未完成。

证据与回滚：`artifacts/linear-local-20261022/installation.json`、`real-verification.json`、`integrated-source-manifest.json`、`installation-backup/`。回滚只恢复二进制，不回灌旧数据库。

当前安装 SHA256：app.asar `595002b776400ccf403b13e3514562edcc8f7e888cbc1097671f8ddf98bafb7e`；runtime `cc75f1d253bfcd1070c648f2b0e17fcbd7e853f99566b091c673a4a9378190b2`。

## 历史扫码候选记录（已被用户的新选择取代，不执行公网部署）

用户结果：在 Velella 点击连接 Linear，通过手机二维码或本机浏览器授权后，自动保存工作区来源并读取任务。API Key 只作为主动选择的备用方式。

总体验收：真实 Linear 工作区授权成功、桌面自动显示来源与任务、确认取消后不产生迟到保存、授权到期可刷新、凭据不进入 UI/日志/Agent。测试、构建和代码审查只是阶段，不能替代真实扫码验收。

基线：D:\orcas，62769b4；开始时工作树干净。当前聊天为本次修改唯一写入者。改动范围为 Linear OAuth 模块、来源凭据处理、连接界面、独立回调服务与专项测试。原安装与已连接的 10 个 GitHub 仓库保持现状。

关键路径：注册 Velella Linear OAuth 应用并确定 HTTPS 回调域名 → 部署独立回调服务 → 配置桌面 Client ID 与服务 origin → 安装候选 → 真实手机/浏览器授权验收。代码与本地测试先完成；部署和替换安装需单独授权。

## 实现

- desktop/src/LinearConnect.tsx：扫码、浏览器、取消、过期、结果未知和手动备用入口；打开弹窗本身不启动 OAuth。
- crates/velella/src/task_sources/linear_login.rs：PKCE S256、受配置约束的 Linear 官方授权 URL、请求幂等与取消、授权交换、工作区识别、任务预读取、事务保存、凭据刷新。
- services/linear-oauth-relay：无第三方依赖的 Node 服务；只暂存短期 authorization code，需私有 poll token 才能读取；不会收到 PKCE verifier 或 API token。
- access/refresh token 使用现有 Windows 用户级 DPAPI 保护。并发刷新串行化，保存使用条件更新，避免覆盖已断开的连接。

## 本地验证

Rust：`cargo test -p velella --lib task_sources --no-fail-fast`。

Relay：`node --test services/linear-oauth-relay/server.test.mjs`。

浏览器：先设置 `VELELLA_LINEAR_QR_TEST_OUTPUT` 为 `artifacts/linear-oauth-20261022/qr-fixture.json` 的绝对路径，运行 Rust 的 `generates_linear_qr_fixture` 测试，再在 desktop 执行 `node tests/linear-connect.browser.mjs`。需要 Playwright 与 Microsoft Edge；本机可通过 NODE_PATH 使用 Codex bundled runtime 的 Playwright，无需修改产品依赖。

前端：在 desktop 执行 `npm run build`。

证据位于 artifacts/linear-oauth-20261022。所有浏览器授权结果均为隔离 fixture。真实 OAuth 应用注册、公网部署、安装、手机扫码和业务 UAT 尚未完成。部署步骤见 services/linear-oauth-relay/README.md。

## 本次结果与冻结

- 实现：扫码/浏览器共用短授权入口；已完成。二维码截图已解码且与授权入口一致。
- 开发验证：来源 Rust 32 项通过、2 项原有忽略项；relay 4 项通过；前端单元 96 项通过；浏览器专项 8 项通过；TypeScript/Vite 构建与 diff 空白检查通过。原有 Rust unused/dead-code 和 Vite 大包警告仍在。
- 独立复核：`/root/linear_oauth_review` 只读审查发现两处竞态，均修复并增加回归测试；`/root/linear_oauth_race_check` 对修复定向只读复核通过。独立复核者未另行运行测试，不称为独立测试通过。
- 修复：最终保存前将连接记录与授权开始时快照比较，拒绝覆盖中途断开或修改；活动授权关闭弹窗前等待取消确认，结果未知时保持弹窗。强制退出进程的结果仍需重新读取连接列表核对。
- 技术状态：本地候选验证通过，按 artifacts/linear-oauth-20261022/verification.json 文件哈希冻结；后续仅新可复现源码问题或需求变化才重开代码。
- 集成/部署/在线/UAT：尚未打包安装此候选，尚未注册真实 Linear OAuth 应用、部署回调服务或执行真实手机授权。10 个现有 GitHub 连接和当前安装未改动。

用户总目标 **未完成**：当前已能审阅扫码界面与可部署服务代码，尚不能在现有安装中使用真实 Linear 扫码。唯一下一阶段是完成应用注册、指定 HTTPS 回调域名并取得部署/安装授权，再执行真实授权验收。
