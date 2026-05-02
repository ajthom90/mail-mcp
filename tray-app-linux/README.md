# mail-mcp-tray (Linux)

GTK4 + libadwaita Linux tray for the mail-mcp daemon. See the design spec at
`docs/superpowers/specs/2026-05-02-v0.1d-linux-tray-design.md` and the
implementation plan at `docs/superpowers/plans/2026-05-02-v0.1d-linux-tray.md`.

## Build

Requires Meson 1.3+, a C11 compiler, and GTK4 / libadwaita / AyatanaAppIndicator3 / json-glib development packages.

Ubuntu 24.04+ / Debian 13+:

```bash
sudo apt install meson ninja-build gcc \
  libgtk-4-dev libadwaita-1-dev \
  libayatana-appindicator3-dev libjson-glib-dev libcmocka-dev
```

Fedora 40+:

```bash
sudo dnf install meson ninja-build gcc \
  gtk4-devel libadwaita-devel \
  libayatana-appindicator3-devel json-glib-devel libcmocka-devel
```

Then:

```bash
meson setup build -Dgoogle_client_id=YOUR_CLIENT_ID
meson compile -C build
meson test    -C build
```

`-Dgoogle_client_id` is optional — when empty, the runtime reads
`MAIL_MCP_GOOGLE_CLIENT_ID` from the environment instead.

## Install

```bash
sudo meson install -C build
```

Installs `mail-mcp-tray` and `mail-mcp-daemon` to `${prefix}/bin/`. To skip
the bundled daemon (e.g. when packaging the daemon separately), pass
`-Dinstall_daemon=false` at configure time.

## Architecture

The tray holds zero persistent state. Every fact comes from the daemon over
the v0.1a Unix-domain socket IPC at `${XDG_RUNTIME_DIR}/mail-mcp/ipc.sock`,
JSON-RPC 2.0 newline-framed.
