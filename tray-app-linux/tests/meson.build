cmocka_dep = dependency('cmocka')

src_inc = include_directories('../src')

test_paths = executable('test_paths',
  'test_paths.c', '../src/paths.c',
  dependencies: [cmocka_dep, gio_dep],
  include_directories: src_inc,
)
test('paths', test_paths)

test_models = executable('test_models',
  'test_models.c', '../src/ipc/models.c',
  dependencies: [cmocka_dep, gio_dep, jsonglib_dep],
  include_directories: src_inc,
)
test('models', test_models)

test_notifications = executable('test_notifications',
  'test_notifications.c', '../src/ipc/models.c', '../src/ipc/notifications.c',
  dependencies: [cmocka_dep, gio_dep, jsonglib_dep],
  include_directories: src_inc,
)
test('notifications', test_notifications)
