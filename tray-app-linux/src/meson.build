sources = files(
  'main.c',
  'paths.c',
  'ipc/models.c',
  'ipc/notifications.c',
  'ipc/ipc_client.c',
)

executable('mail-mcp-tray',
  sources,
  c_args: ['-include', meson.project_build_root() / 'config.h'],
  dependencies: deps,
  install: true,
)
