# VPS Setup For FlyTunnel

FlyTunnel chỉ cần `frps` chạy ổn trên VPS. Phần GUI local sẽ dùng `frpc` để nối vào `frps` và publish Minecraft LAN port ra ngoài.

## 1. Tải frp trên VPS

Ví dụ với Linux x86_64:

```bash
curl -LO https://github.com/fatedier/frp/releases/download/v0.67.0/frp_0.67.0_linux_amd64.tar.gz
tar -xzf frp_0.67.0_linux_amd64.tar.gz
cd frp_0.67.0_linux_amd64
```

Nếu VPS dùng ARM64 thì thay asset tương ứng.

## 2. Tạo `frps.toml`

Dùng file mẫu tại [frps.toml](/D:/Projects/home/FlyTunnel/docs/vps/frps.toml) rồi thay token thật:

```toml
bindPort = 7000

auth.method = "token"
auth.token = "replace-with-a-strong-token"
```

Bạn có thể mở dashboard nội bộ ở `127.0.0.1:7500` hoặc reverse proxy nó sau nếu cần.

## 3. Mở firewall

Mở ít nhất:

- `7000/tcp` cho control connection giữa `frpc` và `frps`
- `25565/tcp` hoặc remote port bạn định cho bạn bè join

Ví dụ với UFW:

```bash
sudo ufw allow 7000/tcp
sudo ufw allow 25565/tcp
sudo ufw reload
```

## 4. Chạy `frps`

```bash
./frps -c ./frps.toml
```

Nếu muốn chạy như service với systemd:

```ini
[Unit]
Description=frp server
After=network.target

[Service]
Type=simple
WorkingDirectory=/opt/frp
ExecStart=/opt/frp/frps -c /opt/frp/frps.toml
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
```

## 5. Kết nối từ FlyTunnel

Trong app FlyTunnel:

- `VPS Host / IP`: IP hoặc domain của VPS
- `Control Port`: `7000`
- `Token`: đúng token trong `frps.toml`
- `Local Port`: cổng LAN Minecraft trên máy host
- `Remote Port`: cổng public bạn bè sẽ join

Khi app báo `Running`, bạn bè có thể join bằng:

```text
VPS_IP:REMOTE_PORT
```

## 6. Debug nhanh

- `invalid token`: token trên app và `frps.toml` không khớp
- `connection refused`: `frps` chưa chạy hoặc firewall chưa mở `7000`
- `port already used`: remote port đang bị dịch vụ khác chiếm
- không ai join được: kiểm tra lại firewall của VPS và confirm đúng `remotePort`
