# FlyTunnel

FlyTunnel là app GUI nhỏ gọn để mở Minecraft LAN world ra internet qua VPS bằng `frp`, không cần mở port router ở nhà. App dùng `Rust + Tauri 2 + vanilla HTML/CSS/JS`, tập trung vào flow đơn giản: nhập VPS, nhập token, bấm `Start Tunnel`, xem log realtime.

## Tính năng v1

- GUI một cửa sổ cho Windows và macOS
- Lưu config local: VPS host, control port, token, local port, remote port, `frpc` override path
- Startup chỉ `probe` binary; chỉ auto-download `frpc` official `v0.67.0` khi thực sự start tunnel
- Fallback chọn `frpc` thủ công nếu auto-download lỗi
- Tạo `frpc.toml` runtime cho Minecraft Java TCP trên `127.0.0.1:<localPort>`
- Start/stop tunnel bằng child process, phân biệt rõ `Starting`, `Running`, `Stopped`, `Error`
- Log realtime từ `stdout/stderr` của `frpc`
- UI nhẹ hơn bản đầu: bỏ blur/shadow nặng, batch log render, giảm save round-trip khi đang gõ
- Kèm bộ deploy Docker + ops kit cho VPS trong [deploy/vps](deploy/vps), README riêng tại [deploy/vps/README.md](deploy/vps/README.md), điều khiển qua [Makefile](Makefile), và guide tại [docs/vps/setup.md](docs/vps/setup.md)

## Cấu trúc

- `src/`: giao diện HTML/CSS/JS
- `src-tauri/`: backend Rust, process manager, config renderer, binary resolver
- `deploy/vps/`: Docker deploy cho `frps` trên VPS Linux
- `Makefile`: local/remote ops cho `frps`
- `docs/vps/`: sample `frps.toml` và hướng dẫn VPS
- `third_party/frp-upstream/`: upstream `frp` clone shallow tại tag `v0.67.0` để tham chiếu local

## Prerequisites

- Node.js 22+
- Rust stable
- Tauri prerequisites theo OS: [tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/)
- Windows build native cần MSVC Build Tools
- macOS build native cần Xcode Command Line Tools và nên build trực tiếp trên Mac

## Chạy local

```bash
npm install
npm run dev
```

Khi bấm `Start Tunnel`, FlyTunnel sẽ:

1. Lưu settings local
2. Kiểm tra `frpc` override path hoặc cache local
3. Auto-download `frpc` nếu cần
4. Tạo runtime config
5. Spawn `frpc -c <path>`
6. Chuyển sang `Running` chỉ sau khi `frpc` báo login success và proxy success

## Test và nghiệm thu local

Chạy full test:

```bash
npm run test
```

`cargo test` hiện bao gồm cả acceptance suite loopback dùng `frps/frpc` official thật. Suite này cover:

- success path với TCP service local giả lập Minecraft
- invalid token
- unreachable `frps`
- remote port conflict
- stop tunnel
- cleanup khi app đóng

Nếu chỉ muốn chạy acceptance suite:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test local_e2e -- --nocapture
```

## Build Windows

Build release:

```bash
npm install
npm run build
```

Artifact chính:

- `src-tauri/target/release/flytunnel.exe`
- `src-tauri/target/release/bundle/nsis/FlyTunnel_0.1.0_x64-setup.exe`

Smoke test local:

1. Mở `flytunnel.exe`
2. Nhập VPS host/IP, control port, token, local port LAN, remote port public
3. Bấm `Start Tunnel`
4. Xem status chuyển `Starting -> Running`
5. Kiểm tra log box và cho bạn bè join bằng `VPS_IP:REMOTE_PORT`

## Build macOS

Nên build trực tiếp trên macOS:

```bash
npm install
npm run build
```

App bundle `.app` và các bundle khác sẽ nằm trong `src-tauri/target/release/bundle/`.

Nếu muốn build per-arch hoặc universal:

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

Workflow CI trong repo cũng build/test cho cả Windows và macOS, rồi upload artifact unsigned.

## VPS setup nhanh

Flow khuyến nghị là Docker trên VPS Linux:

```bash
make vps-init
```

`make vps-init` sẽ tạo sẵn:

- `deploy/vps/.env`
- `deploy/vps/.ssh.env`

Sau khi điền token và thông tin VPS, flow chuẩn sẽ là:

```bash
make vps-config
make vps-remote-deploy
```

Mở firewall `7000/tcp` và `25565/tcp`, sau đó trong app FlyTunnel dùng:

- `Control Port = 7000`
- `Remote Port = 25565`

Nếu muốn chạy local Docker thay vì remote deploy:

```bash
make vps-up
make vps-logs
```

Xem chi tiết tại [docs/vps/setup.md](docs/vps/setup.md).

## Embed `frpc` thay vì auto-download

V1 mặc định dùng auto-download. Nếu muốn ship binary sẵn:

1. Đặt `frpc(.exe)` theo target vào thư mục bundle riêng
2. Thêm `externalBin` trong [src-tauri/tauri.conf.json](src-tauri/tauri.conf.json)
3. Cập nhật `frpc_resolver` để ưu tiên binary bên cạnh app bundle trước khi fallback sang download

Flow resolver đã được tách riêng nên đổi sang bundled binary sau này khá thẳng.
