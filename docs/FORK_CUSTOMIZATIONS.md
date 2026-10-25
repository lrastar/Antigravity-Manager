# 自改分支使用与上游合并

自改仓库为 [lrastar/Antigravity-Manager](https://github.com/lrastar/Antigravity-Manager)，开发分支为 `codex/native-token-throughput-ui`，从上游 `origin/main` 建立。保留上游历史、页面实现和后端 API，不重写 `main`。相关提交记录在 [本 fork 的 #1](https://github.com/lrastar/Antigravity-Manager/issues/1)。

## 启动与使用

在当前已安装依赖的项目目录运行：

```powershell
npm run tauri dev
```

这会启动带有本分支改动的桌面开发版。`npm run dev` 仅启动前端开发服务，不能代替完整桌面启动。此分支交付源码，未生成安装包。

1. 打开“Token 消费统计”，点击“原生统计管理”并开启原生统计。
2. 首次开启会建立历史基线。之后在 Antigravity 原生客户端发起新的对话生成；开启前的历史生成不计入新增统计。
3. 保持管理工具运行，等待后台扫描（每 30 秒），或点击“立即扫描”。原生采集按 Windows Antigravity 2.26.0 的数据结构验收。
4. 在统计页选择“全部”“反代”或“反重力原生”，查看“平均输出速度”及有效样本数。在原生明细中查看每次生成的 `tok/s`。
5. 无法确认账号的记录显示“未归属”，可在原生统计管理中手动归属或取消归属；不会用当前账号猜测历史归属。

反代速度随新请求自动记录，无需开启原生采集。没有可靠耗时的记录不纳入速度平均值，但仍保留 Token 用量。原生用量不计入反代估算成本。

## 页面入口

`src/config/forkFeatures.ts` 集中定义本分支禁用的页面：`apikey-fun`（中转站）、`user-token`（用户 Token）。路由、桌面/移动导航和设置中的菜单开关都使用同一个过滤函数。直接访问禁用页面会由原有兜底路由返回主页。

上游页面、用户 Token API、认证与存量数据保持兼容。恢复入口只需调整这份配置，无需恢复已删除的功能文件。合并上游时保留三个接入点的过滤调用即可。

## 输出速度

统计页的 `tok/s` 为有效计时样本的输出 Token 总和除以耗时总和，包含首字等待，不是去除 TTFT 后的纯解码速度。原生明细也显示每次生成的速度。

- 原生：读取 `gen_metadata` 中 `chat[11]` 与 `chat[12]` 的 protobuf Duration，按秒和纳秒精度相加；输出量使用已有字段，思考不重复相加。
- 反代：在统一监控入口记录请求耗时，不修改四种协议的适配器。旧 `record_usage` 接口保留供上游调用，新接口接受可选耗时。
- 没有可靠耗时的历史记录继续参与用量统计，速度显示不可用，不用扫描时间、文件时间或统计窗口长度估算。
- SQLite 只添加可空的 `duration_ms` 列，不删除或改名原有字段。API 摘要只添加可选 `throughput` 字段；GUI 和 Web 使用相同接口。

原生字段依据与实机只读核验记录见 [token-monitor #955](https://github.com/Javis603/token-monitor/issues/955)。

## 常规合并

当前本地仓库的远程配置为：`origin` 指向上游 `lbjlaq/Antigravity-Manager`，`fork` 指向个人仓库 `lrastar/Antigravity-Manager`。自改分支跟踪 `fork/codex/native-token-throughput-ui`，默认推送到 `fork`。

保持工作区干净，在自改分支上合并上游更新后推送：

```sh
git switch codex/native-token-throughput-ui
git fetch origin main
git merge origin/main
git push fork codex/native-token-throughput-ui
```

在另一台机器上克隆本分支时，可使用以下命令保持相同远程命名（随后安装项目所需的 Node.js、Rust 与平台构建依赖，并运行 `npm ci`）：

```sh
git clone --origin fork --branch codex/native-token-throughput-ui https://github.com/lrastar/Antigravity-Manager.git
cd Antigravity-Manager
git remote add origin https://github.com/lbjlaq/Antigravity-Manager.git
git config remote.pushDefault fork
```

无需将自改历史强推到上游。若发生冲突，保留上游更新，再重新接入集中入口过滤、可空耗时字段与独立速度模块；不要直接整文件采用 ours/theirs。

此结构保留正常合并路径，但无法承诺未来上游修改相同代码时永远零冲突。合并后重点检查入口过滤、历史数据库迁移、Token 来源分离、速度加权计算，以及四种协议的统一用量记录调用。
