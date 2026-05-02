cmocka_dep = dependency('cmocka')

src_inc = include_directories('../src')

test_paths = executable('test_paths',
  'test_paths.c', '../src/paths.c',
  dependencies: [cmocka_dep, gio_dep],
  include_directories: src_inc,
)
test('paths', test_paths)
