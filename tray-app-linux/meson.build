project('mail-mcp-tray', 'c',
  version: '0.1.0',
  default_options: ['c_std=c11', 'warning_level=2', 'werror=true'],
  meson_version: '>=1.3.0')

cc = meson.get_compiler('c')
gnome = import('gnome')

gtk_dep        = dependency('gtk4',                version: '>=4.10')
adw_dep        = dependency('libadwaita-1',        version: '>=1.4')
appind_dep     = dependency('ayatana-appindicator3-0.1')
jsonglib_dep   = dependency('json-glib-1.0')
gio_dep        = dependency('gio-2.0')
gio_unix_dep   = dependency('gio-unix-2.0')

deps = [gtk_dep, adw_dep, appind_dep, jsonglib_dep, gio_dep, gio_unix_dep]

# Build-time Google client ID baked into config.h. When empty, the daemon
# launcher reads MAIL_MCP_GOOGLE_CLIENT_ID from the environment at startup.
google_client_id = get_option('google_client_id')
config = configuration_data()
config.set_quoted('MAIL_MCP_GOOGLE_CLIENT_ID', google_client_id)
config.set_quoted('MAIL_MCP_VERSION', meson.project_version())
configure_file(
  input:  'src/config.h.in',
  output: 'config.h',
  configuration: config,
)

if get_option('install_daemon')
  custom_target('mail-mcp-daemon',
    output: 'mail-mcp-daemon',
    command: [meson.project_source_root() / 'BuildScripts/build-daemon.sh', '@OUTPUT@'],
    install: true,
    install_dir: get_option('bindir'),
    build_by_default: true,
  )
endif

subdir('src')
if get_option('tests')
  subdir('tests')
endif
