# Flow2API 接入指南

网页文档：<http://127.0.0.1:8000/guide>（需管理员登录）。

## 接口契约

- OpenAI Base URL：`http://127.0.0.1:8000/v1`
- `GET /v1/models`：Bearer API Key 认证，返回 `data[]`；新增可选 `capabilities`，包含工作流、图片输入上下限、比例、时长、价格来源。原字段保持兼容。
- `POST /v1/chat/completions`：图片、视频统一入口；`model`、`messages`、`stream`。
- 文字内容：`messages[].content` 可为字符串。
- 参考图：content 数组包含 `text` 和 `image_url`；URL 支持 HTTP(S) 或 data URI。
- 续写：`image_url.url` 使用 `extend://<mediaGenerationId>`。
- 非流式：`choices[0].message.content` 返回 Markdown 图片链接或 HTML video 标签。
- 流式：检查 SSE 的 `error`，合并 `choices[].delta.content`，等待 `[DONE]`，确认存在媒体结果。HTTP 200 不能单独证明生成成功。

## Magpie

已添加本机供应商：`Flow2API`，协议 OpenAI 兼容，地址 `http://127.0.0.1:8000/v1`。

初始模型：
- `flow2api/gemini-3.1-flash-lite-image-square`
- `flow2api/gemini-3.1-flash-image-square`

其他工具调用 Magpie 的 `http://127.0.0.1:3425/v1/chat/completions`，使用 Magpie 网关密钥和上述完整模型 ID。直接调用 Flow2API 时使用 Flow2API API Key，去掉 `flow2api/` 前缀。

不要把媒体模型设为普通编程 Agent 的默认模型；它们不承担普通文本问答和工具调用。先作为图片生成专用调用使用。Flow2API 的视频接口不能直接替换 Magpie Grok 的 `/v1/videos`。

本机接入脚本（先预览，再应用；自动备份，密钥不输出）：
```bash
rtk proxy ./venv/bin/python scripts/connect_magpie.py
rtk proxy ./venv/bin/python scripts/connect_magpie.py --apply
```

## 价格快照

2026-10-19 当前 Flow Pro 页面、单个输出：
- Nano Banana 2 Lite / 2.1 基础图片：0 credits/张。订阅费不包含在此数值中。
- Omni 1.1 Flash 720p：4/6/8/10 秒为 7/10/12/15 credits/条。
- Veo 3.1 Lite/Fast/Quality 8 秒文生视频：10/20/100 credits/条。
- 其他时长、参考图、首尾帧、续写和放大未逐项核价；不推算美元费用。

费用不是服务端实时扣费查询。地区、套餐与 Google 调整可能改变费用；实际扣费以 Flow 为准。

来源：当前登录的 Flow UI；Google credits 说明：<https://support.google.com/googleone?p=g1_ai_credit_menu>。

完整 cURL（含文生图、参考图、视频及 SSE）在网页文档，示例不会包含真实密钥。

## 本次验证（2026-10-19）

- `/v1/models`：82 个登记模型全部携带能力元数据。
- 新工作台真实点击生成图片成功，流式链路耗时 17.1 秒。
- `Magpie → Flow2API → 已登录 ego lite → Flow` 图片链路成功，非流式耗时 15.6 秒；成品已下载并查看。
- 375 / 768 / 1440 宽度：工作台、文档、管理配置页没有整页横向溢出。
- 悬浮信息、模型搜索、能力筛选、首帧缺失校验、配置分组与文档复制按钮已实际操作。
- 12 项 Python 测试与 3 项 SSE 测试通过。SSE 覆盖 HTTP 200 后错误事件、连接中断、跨数据块分隔符。
- 本次未重新测试视频生成或全部模型的上游权限；价格快照不构成扣费保证。

```bash
rtk proxy ./venv/bin/python -m unittest tests.test_model_metadata tests.test_ego_browser_bridge tests.test_browser_executable
rtk proxy node --test tests/studio_stream.test.cjs
rtk proxy ./status.sh
```


## 自动保存与本机资源

所有交付成功的图片（含放大结果）和视频先保存到服务所在电脑的 `data/generated/`，再通过 `/media/<sha256>.<ext>` 返回。工作台、API、Magpie 使用相同流程，无需开启临时缓存。保存失败会返回失败或明确回退为已保存的原图，不会把未保存的远程链接当作成功成品。

- `/library`：登录后查看、下载本机资源；`GET /api/library`：管理员登录会话读取目录和清单。
- `GET /media/<filename>`：预览；加 `?download=true` 下载。文件名由内容 SHA256 得到，不同请求相同文件去重；实际扩展名按文件头识别。
- 媒体链接为持有即能访问的链接，便于其他工具获取，资源清单仍需登录。请保护私密作品链接。
- `data/generated/` 不参与 tmp 超时清理；保留到手动删除。迁移或备份时请一起备份文件与同名 JSON 记录。
- 本机指运行 Flow2API 的电脑；其他设备调用时不会自动写入调用设备的下载目录。

```bash
# 预览 / 下载（把文件名替换为生成结果中返回的实际文件名）
curl --fail 'http://127.0.0.1:8000/media/<实际文件名>?download=true' -o output.jpg
```


### 自动保存实测（2026-10-19）

使用专用 ego lite「Flow2API」工作区（space 14 / p1，复用已有 Google 登录态）在工作台生成图片，耗时 18.4 秒。返回 `/media/` 本机地址，文件 670480 字节、JPEG、1024×1024；HTTP 下载内容与磁盘文件 SHA256 一致。刷新本机资源页后共 6 个资源，新图片正常加载。验证记录：`data/image-smoke-test/archive-verification.json`。本轮验证图片生成与保存，未重新生成视频。

绑定工作区的 Flow 标签页需保持打开，工作区控制权需交给 Agent；关闭或删除后应重新绑定。
