# 自改分支使用与上游合并

自改仓库为 [lrastar/Antigravity-Manager](https://github.com/lrastar/Antigravity-Manager)，开发分支为 `codex/usage-statistics-refactor`，基于已有自改分支继续开发。保留上游历史、页面实现和后端 API，不重写 `main`。相关提交记录在 [本 fork 的 #1](https://github.com/lrastar/Antigravity-Manager/issues/1)。

## Windows 安装包

本分支通过 GitHub Actions 构建 Windows x64 NSIS 安装包。打开 [Windows 构建记录](https://github.com/lrastar/Antigravity-Manager/actions/workflows/build-windows.yml)，选择 `codex/usage-statistics-refactor` 分支已成功的运行，在 Artifacts 中下载 `Antigravity-Manager-Windows-Installer`，解压后运行 `Antigravity-Manager-Setup.exe`。下载 GitHub Artifacts 需要登录 GitHub，产物保留 30 天。

安装前退出正在运行的 Antigravity Tools。安装包已经包含编译后的前后端，不需要在本机安装 Node.js 或 Rust。当前应用版本仍为 `5.0.0`，应用标识沿用上游；内置更新地址也仍指向上游。需要保留本分支自改功能时，后续使用本分支的构建包更新。

构建默认只上传 Artifacts。如需手动重新构建，在已登录 GitHub CLI 的终端运行：

```sh
gh workflow run build-windows.yml --repo lrastar/Antigravity-Manager --ref codex/usage-statistics-refactor -f create_release=false
```

## 开发启动与使用

在当前已安装依赖的项目目录运行：

```powershell
npm run tauri dev
```

这会启动带有本分支改动的桌面开发版。`npm run dev` 仅启动前端开发服务，不能代替完整桌面启动。使用安装包时无需运行此命令，直接启动安装后的应用。

1. 打开“Token 消费统计”，点击“采集设置”并开启原生统计。
2. 首次开启会建立历史基线。之后在 Antigravity 原生客户端发起新的对话生成；开启前的历史生成不计入新增统计。
3. 保持管理工具运行，等待后台扫描（每 30 秒），或点击“立即扫描”。原生采集按 Windows Antigravity 2.26.0 的数据结构验收。
4. 统计页统一显示全部用量，选择日期范围后，在“模型速度”中查看各模型的 `tok/s` 和有效样本数。在原生明细中查看每次生成的速度。
5. 无法确认账号的记录显示“未归属”，可在原生统计管理中手动归属或取消归属；不会用当前账号猜测历史归属。

反代速度随新请求自动记录，无需开启原生采集。没有可靠耗时的记录不纳入速度平均值，但仍保留 Token 用量。

## 用量统计重构

`codex/usage-statistics-refactor` 参照 [CC Switch 用量统计说明](https://github.com/farion1231/cc-switch/blob/main/docs/user-manual/en/4-proxy/4.4-usage.md) 调整统计口径和概览布局：

- “用量概览”显示估算费用、调用数、总 Token、缓存命中四项指标，输入/输出等明细收进“更多指标”。首次进入默认使用概览，已有看板选择继续保留。
- 页面统一展示全部用量，移除来源切换、来源次数拆分及原生采集状态横幅。调用数为反代请求数加原生模型生成次数，历史记录继续保留。
- 日期筛选支持今天、最近 24 小时、最近 7/14/30 天、全部、自定义起止时间。摘要、模型排行、速度及趋势使用同一开始时间（含）和结束时间（不含）；91 天活动图与 24 小时律动保留明确标注的固定周期。
- 三种看板均只展示模型，不显示账号／用户页签及账号排行。后台账号采集、归属及统计接口继续保留，采集设置仍可管理原生记录。
- 以各模型有效输出 Token 总量除以各自有效耗时总和计算速度；缺少可靠耗时的模型显示“—”，用量照常统计。
- 缓存命中率为缓存命中 Token 除以全部输入 Token。后端输入已包含缓存命中和缓存写入，缓存不重复加到总量中。不再按固定单价推测“缓存节省金额”。
- 估算费用 = 非命中输入 × 输入单价 + 缓存命中 × 缓存单价 + 输出 × 输出单价。当前聚合接口不单列缓存写入，因此写入随非命中输入估算；思考 Token 已包含在输出中，不重复计费。这些数值不代表实际账单或剩余额度。
- 价格匹配优先采用具体型号，避免带日期的 mini 等型号误用同系列更宽泛的价格。未匹配型号继续使用配置的默认单价。
- 费用趋势按所选时段各模型的平均 Token 单价分摊，属于近似值；修改单价会重算当前统计，不是历史账单快照。

清空原生统计时会同时重建采集基线，进行中的旧扫描不会重新写回已清空的记录。如果基线读取或写入失败，操作会报错并回滚，保留原统计；启用原生采集时也会先完整建立基线。

## 页面入口

`src/config/forkFeatures.ts` 集中定义本分支禁用的页面：`apikey-fun`（中转站）、`user-token`（用户 Token）。路由、桌面/移动导航和设置中的菜单开关都使用同一个过滤函数。直接访问禁用页面会由原有兜底路由返回主页。

上游页面、用户 Token API、认证与存量数据保持兼容。恢复入口只需调整这份配置，无需恢复已删除的功能文件。合并上游时保留三个接入点的过滤调用即可。

## 输出速度

统计页的 `tok/s` 为有效计时样本的输出 Token 总和除以耗时总和，包含首字等待，不是去除 TTFT 后的纯解码速度。原生明细也显示每次生成的速度。

- 原生：读取 `gen_metadata` 中 `chat[11]` 与 `chat[12]` 的 protobuf Duration，按秒和纳秒精度相加；输出量使用已有字段，思考不重复相加。
- 反代：在统一监控入口记录请求耗时，不修改四种协议的适配器。旧 `record_usage` 接口保留供上游调用，新接口接受可选耗时。
- 没有可靠耗时的历史记录继续参与用量统计，速度显示不可用，不用扫描时间、文件时间或统计窗口长度估算。
- SQLite 只添加可空的 `duration_ms` 列，不删除或改名原有字段。摘要及模型统计使用可选 `throughput` 字段；GUI 的 `get_token_stats_dashboard` 与 Web 的 `/api/stats/token/dashboard` 使用相同聚合实现。

原生字段依据与实机只读核验记录见 [token-monitor #955](https://github.com/Javis603/token-monitor/issues/955)。

## 常规合并

当前本地仓库的远程配置为：`origin` 指向上游 `lbjlaq/Antigravity-Manager`，`fork` 指向个人仓库 `lrastar/Antigravity-Manager`。自改分支跟踪 `fork/codex/usage-statistics-refactor`，默认推送到 `fork`。

保持工作区干净，在自改分支上合并上游更新后推送：

```sh
git switch codex/usage-statistics-refactor
git fetch origin main
git merge origin/main
git push fork codex/usage-statistics-refactor
```

在另一台机器上克隆本分支时，可使用以下命令保持相同远程命名（随后安装项目所需的 Node.js、Rust 与平台构建依赖，并运行 `npm ci`）：

```sh
git clone --origin fork --branch codex/usage-statistics-refactor https://github.com/lrastar/Antigravity-Manager.git
cd Antigravity-Manager
git remote add origin https://github.com/lbjlaq/Antigravity-Manager.git
git config remote.pushDefault fork
```

无需将自改历史强推到上游。若发生冲突，保留上游更新，再重新接入集中入口过滤、可空耗时字段与独立速度模块；不要直接整文件采用 ours/theirs。

此结构保留正常合并路径，但无法承诺未来上游修改相同代码时永远零冲突。合并后重点检查入口过滤、历史数据库迁移、Token 来源分离、速度加权计算，以及四种协议的统一用量记录调用。
