# FlyTunnel VPS Deploy

Deploy này chỉ dành cho `frps` trên VPS Linux. App FlyTunnel desktop vẫn chạy trên máy host Minecraft.

## Nhanh nhất

```bash
make vps-init
```

Điền:

- `deploy/vps/.env`
- `deploy/vps/.ssh.env`

Rồi chạy:

```bash
make vps-config
make vps-remote-deploy
```

## File cần sửa

`deploy/vps/.env`

- `FRPS_TOKEN`
- `FRPS_DASHBOARD_PASSWORD`

`deploy/vps/.ssh.env`

- `VPS_HOST`
- `VPS_USER`
- `VPS_PORT`
- `VPS_PATH`
- `SSH_KEY`

## Cấu hình đã chốt

- `frps` chạy bằng Docker Compose
- `network_mode: host`
- control port `7000`
- dashboard chỉ bind `127.0.0.1:7500`
- chỉ cho phép remote port `25565`
- healthcheck dùng dashboard API nội bộ

## Lệnh hữu ích

Local:

```bash
make vps-build
make vps-up
make vps-status
make vps-logs
make vps-down
```

Remote:

```bash
make vps-remote-preflight
make vps-remote-sync
make vps-remote-deploy
make vps-remote-status
make vps-remote-logs
make vps-remote-restart
make vps-remote-down
```

## App FlyTunnel cần nhập gì

- `VPS Host / IP`: IP hoặc domain VPS
- `Control Port`: `7000`
- `Token`: đúng với `FRPS_TOKEN`
- `Local Port`: cổng LAN world trên máy host
- `Remote Port`: `25565`

Khi tunnel chạy, bạn bè join bằng:

```text
VPS_IP:25565
```

## Firewall

Mở ít nhất:

- `22/tcp`
- `7000/tcp`
- `25565/tcp`

Không cần public `7500` vì dashboard chỉ mở trên loopback.

## Debug nhanh

- `Permission denied (publickey)`: kiểm tra `SSH_KEY` hoặc key chưa add lên provider
- `docker compose config` fail: kiểm tra `.env`
- `health: starting` mãi: kiểm tra dashboard credentials trong `.env`
- `port not allowed`: app đang dùng remote port khác `25565`
- `connection refused`: `frps` chưa lên hoặc firewall chưa mở `7000`
