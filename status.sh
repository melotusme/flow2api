#!/usr/bin/env bash
# 查看 flow2api 本地部署状态与本次改动摘要
cd "$(dirname "$0")" || exit 1

echo "# --- 部署状态 ---"
PID=$(lsof -nP -t -iTCP:8000 -sTCP:LISTEN | head -1)
if [ -n "$PID" ]; then
  echo "[运行中] PID=$PID  http://localhost:8000"
else
  echo "[未运行] 启动: nohup ./start.sh >> /tmp/flow2api-current.log 2>&1 &"
fi
echo

echo "# --- 文件 ---"
for f in venv config/setting.toml data tmp; do
  [ -e "$f" ] && echo "[存在] $f" || echo "[缺失] $f"
done
echo

echo "# --- 健康检查 ---"
curl -fsS -m 5 -o /dev/null -w 'HTTP %{http_code}\n' http://127.0.0.1:8000/login || echo "无响应"
echo

echo
echo "# --- 最近日志 ---"
[ -f /tmp/flow2api-current.log ] && tail -n 15 /tmp/flow2api-current.log || echo "无日志"
