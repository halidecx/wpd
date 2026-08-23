project(
  'wpd',
  'c',
  version: '0.1.0',
  meson_version: '>= 1.4.0',
  default_options: [
    'buildtype=release',
    'c_std=c11',
    'warning_level=2',
    'b_ndebug=if-release',
    'default_library=both',
    'prefer_static=true'
  ],
)

cc = meson.get_compiler('c')
source_root = meson.current_source_dir()
cpu_family = host_machine.cpu_family()

public_header = import('fs').read('include/wpd.h')
header_version = []
foreach line : public_header.split('\n')
  foreach part : ['MAJOR', 'MINOR', 'PATCH']
    if line.startswith('#define WPD_VERSION_' + part + ' ')
      header_version += line.split(' ')[2].strip()
    endif
  endforeach
endforeach
if '.'.join(header_version) != meson.project_version()
  error('include/wpd.h says @0@, project() says @1@'.format(
    '.'.join(header_version), meson.project_version()))
endif
if not public_header.contains(
    '#define WPD_VERSION_STR "' + meson.project_version() + '"')
  error('WPD_VERSION_STR in include/wpd.h is not ' + meson.project_version())
endif

optional_c_args = [
  '-fvisibility=hidden',
  '-Werror=vla',
  '-Werror=missing-prototypes',
  '-Wshorten-64-to-32',
]
if get_option('buildtype') not in ['debug', 'plain']
  optional_c_args += '-fomit-frame-pointer'
endif
add_project_arguments(cc.get_supported_arguments(optional_c_args), language: 'c')

getopt_sources = []
getopt_inc_dirs = []
if not cc.has_function('getopt_long', prefix: '#include <getopt.h>')
  getopt_sources = files('tools/compat/getopt.c')
  getopt_inc_dirs = include_directories('include/compat')
endif

inc_dirs = [include_directories('.', 'include', 'src')]
lib_c_args = ['-Wno-unused-parameter', '-DWPD_BUILDING']

asm_enabled = get_option('enable_asm') and (
  cpu_family == 'aarch64' or
  cpu_family.startswith('arm') or
  cpu_family.startswith('x86')
)

asm_c_args = ['-DWPD_HAVE_ASM=@0@'.format(asm_enabled ? 1 : 0)]
cpu_baseline_c_args = []
lib_c_args += asm_c_args

trim_dsp = get_option('trim_dsp') == 'true' or (
  get_option('trim_dsp') == 'if-release' and
  get_option('buildtype') in ['release', 'minsize'])
trim_c_args = trim_dsp ? ['-DWPD_TRIM_DSP_FUNCTIONS=1'] : []

if asm_enabled
  if cpu_family.startswith('x86')
    nasm = find_program('nasm')
  elif cpu_family == 'aarch64'
    inc_dirs += [include_directories('src/aarch64')]
  elif cpu_family.startswith('arm')
    inc_dirs += [include_directories('src/arm')]

    if cc.compiles(
      '''#if !defined(__ARM_ARCH) || __ARM_ARCH < 6
         int main(void) { return 0; }''',
      name: 'ARMv6 target')
      cpu_baseline_c_args += ['-DWPD_ARM_ARMV6_ASM=1']
    endif
  endif
endif

lib_c_args += cpu_baseline_c_args

message('Architecture assembly: ' + (asm_enabled ? 'enabled' : 'disabled'))
message('Trim DSP functions: ' + (trim_dsp ? 'enabled' : 'disabled'))

cargo = find_program('cargo')
cargo_features = []
if asm_enabled
  cargo_features += 'asm'
endif
if trim_dsp
  cargo_features += 'trim_dsp'
endif
if get_option('force_rac32')
  cargo_features += 'force_rac32'
endif
threads_enabled = not get_option('threads').disabled()
if threads_enabled
  cargo_features += 'threads'
endif

message('Threads: ' + (threads_enabled ? 'enabled' : 'disabled'))

cargo_profile = {
  'release': 'release',
  'debugoptimized': 'debugopt',
  'minsize': 'minsize',
  'debug': 'debug',
  'plain': 'release',
}.get(get_option('buildtype'), 'release')

cargo_env = environment()
if get_option('nightly_size')
  if get_option('buildtype') != 'minsize'
    error('nightly_size requires buildtype=minsize')
  endif
  cargo_env.set('RUSTUP_TOOLCHAIN', 'nightly')
  cargo_env.set('WPD_NIGHTLY_SIZE', '1')
endif
if get_option('b_sanitize') != 'none'
  cargo_env.append('CFLAGS', '-fsanitize=' + get_option('b_sanitize'),
                   '-fno-omit-frame-pointer', separator: ' ')
  cargo_env.set('WPD_SANITIZE', get_option('b_sanitize'))
endif
if meson.is_cross_build()
  # Cargo defaults to the build machine, so a cross build would link a
  # wrong-architecture staticlib and every Rust symbol would come up
  # undefined.  Hand the wrappers the host triple instead.
  rust_cpu = {
    'x86': 'i686',
    'arm': 'armv7',
  }.get(cpu_family, cpu_family)
  # rustc has no aarch64-pc-windows-gnu: gnullvm is the GNU-flavoured
  # aarch64 Windows target, and 32-bit arm has no Windows target at all.
  rust_windows = {
    'aarch64': 'pc-windows-gnullvm',
    'arm': '',
  }.get(cpu_family, 'pc-windows-gnu')
  rust_system = {
    'darwin': 'apple-darwin',
    'linux': 'unknown-linux-gnu' + (cpu_family == 'arm' ? 'eabihf' : ''),
    'windows': rust_windows,
  }.get(host_machine.system(), '')
  if rust_system == ''
    error('no Rust target triple mapping for ' + cpu_family + ' ' +
          host_machine.system())
  endif
  cargo_env.set('WPD_CARGO_TARGET', rust_cpu + '-' + rust_system)
endif
soversion = meson.project_version()

libwpd_artifacts = custom_target(
  'libwpd',
  output: ['libwpd.a'],
  command: [
    files('tools/cargo_build.sh'),
    source_root,
    meson.current_build_dir(),
    'wpd',
    cargo_profile,
    cargo_features,
  ],
  env: cargo_env,
  build_always_stale: true,
  build_by_default: true,
  install: true,
  install_dir: get_option('libdir'),
)
libwpd = libwpd_artifacts[0]

wpd_symbols = run_command(
  files('tools/gen_exports.sh'),
  source_root / 'include/wpd.h',
  'names',
  check: true,
).stdout().strip().split('\n')

if host_machine.system() == 'darwin'
  export_format = 'macho'
  symbol_prefix = '_'
  export_flag = '-Wl,-exported_symbols_list,'
else
  export_format = 'elf'
  symbol_prefix = ''
  export_flag = '-Wl,--version-script='
endif

wpd_exports = configure_file(
  output: 'wpd.exports',
  command: [
    files('tools/gen_exports.sh'),
    source_root / 'include/wpd.h',
    export_format,
  ],
  capture: true,
)

undefined_args = []
foreach s : wpd_symbols
  undefined_args += '-Wl,-u,' + symbol_prefix + s
endforeach

libwpd_sealed = disabler()
if get_option('nightly_size')
  libwpd_sealed_artifacts = custom_target(
    'libwpd_sealed',
    output: ['libwpd-sealed.a'],
    command: [
      files('tools/seal_archive.sh'),
      libwpd,
      '@OUTPUT@',
      export_format,
      wpd_symbols,
    ],
    depends: libwpd_artifacts,
    build_by_default: true,
    install: true,
    install_dir: get_option('libdir'),
  )
  libwpd_sealed = libwpd_sealed_artifacts[0]
endif

if cargo_profile in ['release', 'minsize']
  if host_machine.system() == 'darwin'
    gc_args = cc.get_supported_link_arguments('-Wl,-dead_strip')
    strip_args = cc.get_supported_link_arguments('-Wl,-x')
  else
    gc_args = cc.get_supported_link_arguments('-Wl,--gc-sections')
    strip_args = cc.get_supported_link_arguments('-Wl,-s')
  endif
else
  gc_args = []
  strip_args = []
endif

libwpd_shared = shared_library(
  'wpd',
  files('src/shared_stub.c'),
  link_args: undefined_args + gc_args + strip_args + [
    export_flag + meson.current_build_dir() / 'wpd.exports',
    meson.current_build_dir() / 'libwpd.a',
  ],
  link_depends: [libwpd_artifacts, wpd_exports],
  dependencies: [
    cc.find_library('m', required: false),
    dependency('threads', required: false),
    cc.find_library('dl', required: false),
  ],
  version: soversion,
  soversion: soversion.split('.')[0],
  install: true,
)

install_headers('include/wpd.h')

import('pkgconfig').generate(
  libwpd_shared,
  name: 'wpd',
  filebase: 'wpd',
  description: 'A fast WebP decoder',
  version: meson.project_version(),
)

wpd_dep = declare_dependency(
  link_with: libwpd,
  include_directories: inc_dirs,
  compile_args: asm_c_args + cpu_baseline_c_args + trim_c_args + ['-DWPD_STATIC'],
  dependencies: [
    cc.find_library('m', required: false),
    dependency('threads', required: false),
    cc.find_library('dl', required: false),
  ],
)

if get_option('nightly_size')
  wpd_sealed_dep = declare_dependency(
    link_with: libwpd_sealed,
    include_directories: inc_dirs,
    compile_args: asm_c_args + cpu_baseline_c_args + trim_c_args + ['-DWPD_STATIC'],
    dependencies: [
      cc.find_library('m', required: false),
      dependency('threads', required: false),
      cc.find_library('dl', required: false),
    ],
  )
endif

wpd = custom_target(
  'wpd-tool',
  output: 'wpd',
  command: [
    files('tools/cargo_tool.sh'),
    source_root,
    meson.current_build_dir(),
    cargo_profile,
    cargo_features,
  ],
  env: cargo_env,
  depends: libwpd_artifacts,
  build_always_stale: true,
  build_by_default: true,
  install: false,
)

api_test = executable(
  'test_api',
  files('tests/api.c'),
  dependencies: [wpd_dep, cc.find_library('m', required: false)],
  install: false,
)
test('api', api_test, suite: 'api')

test(
  'cargo',
  cargo,
  args: [
    'test',
    '--manifest-path', source_root / 'Cargo.toml',
    '--target-dir', meson.current_build_dir() / 'cargo-test',
    '--workspace',
    '--no-default-features',
    '--features', ','.join(cargo_features),
  ],
  env: cargo_env,
  suite: 'cargo',
  timeout: 600,
)

if get_option('nightly_size')
  api_sealed_test = executable(
    'test_api_sealed',
    files('tests/api.c'),
    dependencies: [wpd_sealed_dep, cc.find_library('m', required: false)],
    install: false,
  )
  test('api_sealed', api_sealed_test, suite: 'api')
endif

fuzz_test = executable(
  'test_fuzz',
  files('tests/fuzz.c'),
  dependencies: wpd_dep,
  build_by_default: false,
  install: false,
)

if cc.has_header('sys/mman.h')
  rowbounds_test = executable(
    'test_rowbounds',
    files('tests/rowbounds.c'),
    dependencies: wpd_dep,
    c_args: lib_c_args,
    install: false,
  )
  test('rowbounds', rowbounds_test, suite: 'rowbounds')
endif

libwebp_source = get_option('libwebp')
libwebpdecoder_path = get_option('libwebpdecoder')
libwebp_dec_dep = declare_dependency()
libwebpdec_deps = []
have_libwebp = false
libwebp_origin = ''

if libwebpdecoder_path != ''
  libwebp_dec_dep = declare_dependency(link_args: [libwebpdecoder_path])
  have_libwebp = true
  libwebp_origin = libwebpdecoder_path
elif libwebp_source in ['auto', 'subproject']
  libwebp_cmake_options = import('cmake').subproject_options()
  libwebp_cmake_options.add_cmake_defines({
    'BUILD_SHARED_LIBS': false,
    'WEBP_BUILD_ANIM_UTILS': false,
    'WEBP_BUILD_CWEBP': false,
    'WEBP_BUILD_DWEBP': false,
    'WEBP_BUILD_EXTRAS': false,
    'WEBP_BUILD_GIF2WEBP': false,
    'WEBP_BUILD_IMG2WEBP': false,
    'WEBP_BUILD_LIBWEBPMUX': false,
    'WEBP_BUILD_VWEBP': false,
    'WEBP_BUILD_WEBPINFO': false,
    'WEBP_BUILD_WEBPMUX': false,
  })
  libwebp_subproject = import('cmake').subproject(
    'libwebp',
    options: libwebp_cmake_options,
    required: libwebp_source == 'subproject',
  )
  if libwebp_subproject.found()
    libwebp_dec_dep = libwebp_subproject.dependency('webpdecoder')
    libwebpdec_deps = [
      libwebp_subproject.dependency('webpdemux'),
      libwebp_dec_dep,
    ]
    have_libwebp = true
    libwebp_origin = 'subproject'
  endif
endif

if not have_libwebp and libwebp_source in ['auto', 'system']
  libwebp_dec_dep = dependency('libwebp', required: false, static: true)
  have_libwebp = libwebp_dec_dep.found()
  libwebp_origin = 'system'
endif

if libwebpdec_deps.length() == 0 and have_libwebp
  libwebpdemux_dep = dependency('libwebpdemux', required: false, static: true)
  if libwebpdemux_dep.found()
    libwebpdec_deps = [libwebp_dec_dep, libwebpdemux_dep]
  endif
endif

if have_libwebp
  parity_test = executable(
    'test_parity',
    files('tests/parity.c'),
    dependencies: [
      wpd_dep,
      libwebp_dec_dep,
      cc.find_library('m', required: false),
    ],
    install: false,
  )
  message('parity: enabled, against libwebp from ' + libwebp_origin)
else
  parity_test = disabler()
  message('parity: disabled (no libwebp)')
endif

if libwebpdec_deps.length() > 0
  executable(
    'libwebpdec',
    'tools/libwebpdec.c',
    getopt_sources,
    dependencies: libwebpdec_deps,
    include_directories: getopt_inc_dirs,
    build_by_default: false,
    install: false,
  )
  message('libwebpdec: enabled (build explicitly with target libwebpdec)')
else
  message('libwebpdec: disabled (libwebp demuxer not found)')
endif

custom_target(
  'imagewebpdec',
  output: 'imagewebpdec',
  command: [
    files('tools/cargo_imagewebpdec.sh'),
    source_root,
    meson.current_build_dir(),
    cargo_profile,
  ],
  env: cargo_env,
  build_always_stale: true,
  build_by_default: false,
  install: false,
)
message('imagewebpdec: enabled (build explicitly with target imagewebpdec)')

checkasm_dep = dependency(
  'checkasm',
  fallback: ['checkasm', 'checkasm_dep'],
  required: false,
)

if asm_enabled and checkasm_dep.found()
  checkasm_features = []
  foreach f : cargo_features
    if f != 'trim_dsp'
      checkasm_features += f
    endif
  endforeach

  libwpd_checkasm = custom_target(
    'libwpd_checkasm',
    output: 'libwpd_checkasm.a',
    command: [
      files('tools/cargo_build.sh'),
      source_root,
      meson.current_build_dir(),
      'wpd_checkasm',
      cargo_profile,
      checkasm_features,
    ],
    env: cargo_env,
    build_always_stale: true,
    build_by_default: false,
    install: false,
  )

  wpd_checkasm_dep = declare_dependency(
    link_with: libwpd_checkasm,
    include_directories: inc_dirs,
    dependencies: [
      cc.find_library('m', required: false),
      dependency('threads', required: false),
      cc.find_library('dl', required: false),
    ],
  )

  checkasm = executable(
    'checkasm',
    files(
      'tests/checkasm/main.c',
      'tests/checkasm/filters.c',
      'tests/checkasm/lossless.c',
      'tests/checkasm/rescaler.c',
      'tests/checkasm/vp8dsp.c',
      'tests/checkasm/vp8pred.c',
      'tests/checkasm/yuvdsp.c',
    ),
    dependencies: [wpd_checkasm_dep, checkasm_dep],
    include_directories: include_directories('tests/checkasm'),
    c_args: lib_c_args,
    build_by_default: true,
    install: false,
  )
  test('checkasm', checkasm, suite: 'checkasm', timeout: 180)
  benchmark('checkasm', checkasm, suite: 'checkasm', timeout: 3600,
            args: '--bench')
endif

subdir('tests')
