# GitCode 发布操作

目标公开仓库为 <https://gitcode.com/Wemsur/Celestial-Launcher>，Git 推送地址为 `https://gitcode.com/Wemsur/Celestial-Launcher.git`。首次运行前创建该公开仓库（允许为空），确认账号可推送 tag、创建 release 和上传附件，且匿名用户可访问发布页及附件。

## 配置与运行

在 GitHub 仓库 Actions secrets 配置：

- `GITCODE_USERNAME`：有目标仓库写权限的 GitCode 用户名。
- `GITCODE_TOKEN`：该用户的 GitCode PAT，需具备 Git HTTPS 推送及 release/附件 API 权限。
- 继续使用现有 `TAURI_SIGNING_PRIVATE_KEY` 和 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`；无需更换签名密钥。

手动运行 **Celestial release**，选择所需平台及分发目标。`publish_gitcode` 默认开启，替代原 Wemgit 选项；GitHub 和 git.gay 仍分别生成草稿。推送 `v*` tag 仍只触发构建，不触发发布。

GitCode 任务下载本次所选平台的已签名产物，以 Node.js 22 运行 `.github/scripts/publish-gitcode.mjs release-files`。脚本要求 `GITCODE_USERNAME`、`GITCODE_TOKEN`、`RELEASE_TAG`、`RELEASE_VERSION`、`RELEASE_COMMIT` 环境变量；检出必须是完整历史且 HEAD 与构建提交一致。

## 不构建的分发测试

在 GitHub Actions 手动运行 **GitCode distribution smoke test**，选择包含该工作流的分支。使用同一仓库的 `GITCODE_USERNAME`、`GITCODE_TOKEN`，不需要签名私钥、不安装项目依赖、不编译启动器。

工作流生成不含凭据的小文本 `connectivity-test.txt`，依次验证仓库 API、Git tag 查询/推送、公开预发布创建、OBS 附件上传、附件登记及匿名下载 SHA256。Git 操作复用正式发布器的诊断与认证；实际下载使用客户端同类主机白名单，每跳检查，不携带 Token。若实际下载主机不符合白名单，会输出去除查询串的地址并停止，不能据此放开任意云存储。

每次 tag 为 `distribution-test-<run_id>-<run_attempt>`，不使用 `v*`，不触发本仓库的版本构建。始终为公开 `pre`，不推广为 latest，文本也不匹配安装包名。推送 tag 会同步当前提交可达的源码历史，与正式分发相同。

测试通过或失败都查看运行摘要：可能已产生测试 tag 或预发布；按摘要链接手动删除测试 Release，再单独删除同名 tag。脚本不自动删除、不覆盖已有发布。这个测试验证传输和公开下载，不验证安装包签名，也不替代真实安装包验收。本次代码交付没有执行这个测试工作流。

## 暂存与人工推广

GitCode 没有 draft API。脚本始终以 `release_status: pre` 创建**公开预发布**，包括稳定版本号；上传期间可能已经能看到不完整附件，不能视为私有草稿。

脚本先检查每个安装包都有同名 `.sig`，只接受当前版本的工作流安装包名及其签名；目录必须平铺，拒绝符号链接及其他文件。最多 64 个文件，单个安装包不超过 4 GiB、签名不超过 64 KiB，总计不超过 16 GiB，且文件均非空。这是签名配对检查，不是密码学验签；签名来自既有构建签名步骤。

检查不存在同名 release 后，将**实际构建提交**推送为 GitCode tag；同名远端 tag 必须解析到相同提交，否则停止。不强推、不跳过 hooks，不推送分支。即使仓库为空也可推送；为了创建 release target，这会同步该提交可达的源代码和历史，请先确认可公开。

上传通过 GitCode API 获取 OBS 临时地址，直接流式 PUT；仅接受 HTTPS、默认 443、无用户信息/fragment 的精确主机 `file.gitcode.com` 或 Huawei OBS 公有对象存储主机及指定四个响应头，并按已核验文件大小发送 Content-Length，不向 OBS 发送 GitCode PAT，不跟随重定向。API 使用 Bearer 认证，并限制响应大小及请求时长；日志不输出 PAT、临时签名查询串或原始响应。

全部上传后，脚本重新读取 release，核对每个附件名唯一且有 HTTPS 下载地址，并在 API 提供时核对大小及 SHA-256/SHA-512 摘要。Actions summary 提供发布链接。维护者核查附件完整、签名齐全及匿名下载后，在 GitCode 发布页**人工推广**正式版本；测试版本可继续保持预发布。脚本不调用推广 API，即使稳定 tag 也不会自动改为 `latest`。

## 客户端来源与旧版本过渡

本次客户端正式更新顺序为 GitCode → git.gay → GitHub；原 GitHub 测试 Release 功能开关保持默认关闭及原有指定测试 tag。GitCode 元数据只接受明确的 pre/latest 状态与布尔 prerelease，并把附件转换为官方固定下载入口；缺少对应 `.sig` 时尝试下一个更新源。公告顺序为 GitCode main → GitHub published，匿名读取 `https://api.gitcode.com/api/v5/repos/Wemsur/celestial-announcement/raw/index.json?ref=main`，不向客户端放入 PAT。公告数据库结构、ID、generation 和“不再显示”偏好均不重置。

旧客户端不会自动得知新来源。把包含本次修改的过渡版本同时发布到旧客户端认识的 GitHub/git.gay 正式来源；不要只发布在 GitCode。旧代码在高优先源返回较旧但有效版本时会停止回退，因此旧 wemgit 若恢复后仍提供过时的成功响应，可能挡住旧客户端升级，应维护过渡发布或明确提供不可用响应并引导手动下载。

## 客户端下载与上线验收

配套客户端采用官方固定入口 `https://api.gitcode.com/api/v5/repos/Wemsur/Celestial-Launcher/releases/:tag/attach_files/:filename/download`，tag 和文件名分别 URL 编码。签名入口把 `:filename` 替换为原安装包文件名加 `.sig`，仍以 `/download` 结尾；不要拼成 `/download.sig`。

客户端元数据归一化要求 `release_status` 为 `pre` 或 `latest`，并生成布尔类型的 `prerelease`。稳定更新只接收 `latest`，因此稳定 tag 暂存成功后仍需人工推广，不能仅凭版本号判断已经面向稳定客户端发布。

**首次真实发布必须线上验收实际下载重定向。** 当前没有可探测的发布，尚未确认真实 OBS bucket。上传白名单检查和 release 元数据核验不代表客户端下载链已经验收：维护者需分别匿名访问安装包和 `.sig` 的官方固定入口，核查每一跳 HTTPS 主机、最终可下载内容及客户端验签成功；记录域名时去掉签名查询串。客户端仅允许 GitCode/AtomGit 命名的 OBS bucket 与区域 `myhuaweicloud.com` 域名，不得为排障放开任意云存储域名或关闭验签。若实际域名不符合该限制，应暂停推广，确认来源后另行调整精确白名单。

## 失败、重跑与回退

脚本拒绝任何已有同名 release，不覆盖、不删除远端附件。创建后上传失败可能留下部分附件及公开预发布；请勿推广。重跑前必须人工核实并处理已有 release（例如确认后删除失败暂存 release，或采用新版本/tag 重新构建）。仅在 release 不存在且 tag 指向同一构建提交时允许复用该 tag；冲突 tag 需人工解决，脚本不会强制替换。网络超时也可能已经完成远端写入，先检查 GitCode 再重跑。

GitCode 失败不会撤销其他站点的发布。可取消勾选 `publish_gitcode`，保留 GitHub/git.gay 分发。已存在的客户端仍按其内置旧来源查询更新，本次工作流不修改客户端源配置或迁移旧域名；需要继续维护旧来源或由维护者另行安排客户端更新，新增 GitCode release 本身不会迁移旧客户端。
