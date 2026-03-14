# VPS Setup For FlyTunnel

FlyTunnel chỉ cần `frps` chạy ổn trên VPS. Flow khuyến nghị cho v1 là deploy `frps` bằng Docker trên VPS Linux, điều khiển bằng `make`, còn app FlyTunnel desktop vẫn chạy ở máy host Minecraft.

## 1. Chuẩn bị trên VPS

Cần tối thiểu:

- Docker Engine
- Docker Compose plugin
- local machine có `make`, `ssh`, `rsync` nếu muốn dùng remote deploy tiện lợi
- firewall cho phép `7000/tcp` và `25565/tcp`

Ví dụ với UFW:

```bash
sudo ufw allow 7000/tcp
sudo ufw allow 25565/tcp
sudo ufw reload
```

## 2. Tạo sẵn env và SSH config

Từ repo này:

```bash
make vps-init
```

Lệnh này sẽ tạo nếu còn thiếu:

- `deploy/vps/.env`
- `deploy/vps/.ssh.env`

Mở `deploy/vps/.env` và thay ít nhất:

- `FRPS_TOKEN`
- `FRPS_DASHBOARD_PASSWORD`

Khi đã có VPS, mở `deploy/vps/.ssh.env` và điền:

- `VPS_HOST`
- `VPS_USER` nếu không dùng `root`
- `VPS_PORT` nếu SSH không ở cổng `22`
- `VPS_PATH` nếu muốn deploy sang thư mục khác
- `SSH_KEY` nếu không dùng SSH agent mặc định

## 3. Render config và deploy

Kiểm tra config local:

```bash
make vps-config
```

Deploy lên VPS:

```bash
make vps-remote-deploy
```

Compose hiện được chốt theo các lựa chọn sau:

- `network_mode: host` cho VPS Linux
- `bindPort = 7000`
- dashboard chỉ bind `127.0.0.1:7500`
- `allowPorts` chỉ cho phép `25565`

Kiểm tra nhanh:

```bash
make vps-remote-status
make vps-remote-logs
```

Dashboard nếu cần xem trên chính VPS:

```bash
curl http://127.0.0.1:7500
```

Nếu muốn chạy local Docker thay vì remote deploy:

```bash
make vps-up
make vps-status
make vps-logs
```

## 4. Cấu hình app FlyTunnel

Trong app FlyTunnel trên máy host Minecraft:

- `VPS Host / IP`: IP hoặc domain của VPS
- `Control Port`: `7000`
- `Token`: đúng giá trị `FRPS_TOKEN`
- `Local Port`: cổng LAN Minecraft trên máy host
- `Remote Port`: `25565`

Khi app báo `Running`, bạn bè join bằng:

```text
VPS_IP:25565
```

## 5. File deploy trong repo

- [deploy/vps/README.md](../../deploy/vps/README.md)
- [Makefile](../../Makefile)
- [Dockerfile](../../deploy/vps/Dockerfile)
- [docker-compose.yml](../../deploy/vps/docker-compose.yml)
- [frps.toml.template](../../deploy/vps/frps.toml.template)
- [.env.example](../../deploy/vps/.env.example)
- [.ssh.env.example](../../deploy/vps/.ssh.env.example)

Template render ra cấu hình `frps` với token từ env, dashboard local-only, log ra console, và chỉ mở đúng remote port `25565`.

## 6. Manual fallback không dùng Docker

Nếu muốn chạy `frps` thủ công hoặc bằng systemd, có thể dùng file mẫu [frps.toml](frps.toml):

```bash
./frps -c ./frps.toml
```

Với systemd, `log.to = "console"` sẽ đi vào journal của service.

## 7. Debug nhanh

- `invalid token`: token trên app và `FRPS_TOKEN` không khớp
- `connection refused`: container chưa chạy hoặc firewall chưa mở `7000`
- `port not allowed`: app đang dùng `Remote Port` khác `25565`
- `port already used`: trên VPS đã có dịch vụ khác chiếm `25565`
- không ai join được: kiểm tra lại firewall VPS và confirm app đang dùng đúng `Remote Port = 25565`
- `make: command not found` hoặc `rsync: command not found`: dùng Linux/macOS/WSL hoặc cài đủ tool local trước khi remote deploy
