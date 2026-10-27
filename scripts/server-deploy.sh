#!/bin/bash
# yunting 服务端一键部署脚本（VLESS-Reality, sing-box 1.11.15 官方完整版）
# 在全新 Ubuntu/Debian VPS 上以 root 运行: bash server-deploy.sh [端口]
# 产出: 直接可粘进 yunting 的 vless:// 链接
#
# 配方来自 2026-10-25 实战验证:
#   - 必须用官方完整版 sing-box(带 with_reality_server 标签; 一键脚本的精简编译版会导致认证永远失败)
#   - 1.11.15 (LTS, 服务端 Reality 实现稳定)
#   - 回落/SNI 用 www.cloudflare.com (微软域名在该环境有 DNS/v6 拖累; IP 作 SNI 有 uTLS 匹配问题)
set -e

PORT="${1:-443}"
SB_VER="1.11.15"
SB=/usr/local/bin/sing-box

[ "$(id -u)" = 0 ] || { echo "请用 root 运行: sudo bash $0"; exit 1; }
ARCH=$(uname -m); [ "$ARCH" = "x86_64" ] && A=amd64 || A=arm64

echo "== 1/6 安装 sing-box $SB_VER (官方完整版) =="
if ! $SB version 2>/dev/null | grep -q "$SB_VER"; then
  curl -sL -o /tmp/sb.tar.gz "https://github.com/SagerNet/sing-box/releases/download/v${SB_VER}/sing-box-${SB_VER}-linux-${A}.tar.gz"
  tar -xzf /tmp/sb.tar.gz -C /tmp
  [ -f "$SB" ] && cp "$SB" "$SB.bak-$(date +%s)" || true
  cp "/tmp/sing-box-${SB_VER}-linux-${A}/sing-box" "$SB"
  chmod +x "$SB"
fi
# 关键校验: 必须带 with_reality_server 标签
TAGS=$($SB version | grep -i tags || true)
echo "$TAGS" | grep -q with_reality_server || { echo "!! 该二进制缺 with_reality_server 标签(精简版), Reality 无法工作, 已中止"; exit 1; }
$SB version | head -1

echo "== 2/6 生成凭据 =="
UUID=$($SB generate uuid)
PAIR=$($SB generate reality-keypair)
PRIV=$(echo "$PAIR" | grep -i privatekey | awk '{print $2}')
PUB=$(echo "$PAIR"  | grep -i publickey  | awk '{print $2}')
SID=$($SB generate rand --hex 4)
echo "   uuid=$UUID sid=$SID"

echo "== 3/6 写入配置 (端口 $PORT, cloudflare 回落) =="
mkdir -p /etc/sing-box /var/lib/sing-box
cat > /etc/sing-box/config.json <<EOF
{
  "log": { "level": "info", "timestamp": true },
  "inbounds": [
    {
      "type": "vless",
      "tag": "vless-in",
      "listen": "0.0.0.0",
      "listen_port": $PORT,
      "users": [ { "uuid": "$UUID", "flow": "xtls-rprx-vision" } ],
      "tls": {
        "enabled": true,
        "server_name": "www.cloudflare.com",
        "reality": {
          "enabled": true,
          "handshake": { "server": "www.cloudflare.com", "server_port": 443 },
          "private_key": "$PRIV",
          "short_id": [ "$SID" ]
        }
      }
    }
  ],
  "outbounds": [ { "type": "direct", "tag": "direct" } ]
}
EOF
$SB check -C /etc/sing-box

echo "== 4/6 systemd 服务 =="
cat > /etc/systemd/system/sing-box.service <<'EOF'
[Unit]
Description=sing-box service (VLESS-Reality)
Documentation=https://sing-box.sagernet.org
After=network.target nss-lookup.target network-online.target

[Service]
ExecStart=/usr/local/bin/sing-box -D /var/lib/sing-box -C /etc/sing-box run
ExecReload=/bin/kill -HUP $MAINPID
Restart=on-failure
RestartSec=10s
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
EOF
systemctl daemon-reload
systemctl enable --now sing-box
sleep 1
systemctl is-active sing-box >/dev/null || { journalctl -u sing-box -n 20 --no-pager; exit 1; }

echo "== 5/6 防火墙放行 =="
if command -v ufw >/dev/null; then ufw allow "$PORT/tcp" >/dev/null 2>&1 || true; fi
echo "   (云商安全组/Azure NSG 请手动放行 TCP $PORT)"

echo "== 6/6 完成，分享链接如下 =="
IP=$(curl -s4 --max-time 8 https://api.ip.sb/ip || curl -s4 --max-time 8 http://ifconfig.me)
IP=$(echo "$IP" | tr -d '[:space:]')
echo ""
echo "vless://${UUID}@${IP}:${PORT}?encryption=none&flow=xtls-rprx-vision&security=reality&sni=www.cloudflare.com&fp=chrome&pbk=${PUB}&sid=${SID}&type=tcp#yt-$(hostname)"
echo ""
echo "yunting → 导入节点 → 粘贴上面链接 → 一键连接"
