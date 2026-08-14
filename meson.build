project(
  'wpd',
  'c',
  version: '0.1.0',
  meson_version: '>= 0.57.0',
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

# The public header states the version so a consumer can test it at compile
# time, which only helps if it agrees with the one the build ships.
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

lib_c_args += '-DWPD_HAVE_GETAUXVAL=@0@'.format(
  cc.has_function('getauxval', prefix: '#include <sys/auxv.h>') ? 1 : 0)
lib_c_args += '-DWPD_HAVE_ELF_AUX_INFO=@0@'.format(
  cc.has_function('elf_aux_info', prefix: '#include <sys/auxv.h>') ? 1 : 0)

if get_option('force_rac32')
  lib_c_args += '-DWPD_FORCE_RAC32'
endif

asm_enabled = get_option('enable_asm') and (
  cpu_family == 'aarch64' or
  cpu_family.startswith('arm') or
  cpu_family.startswith('x86')
)

asm_c_args = ['-DWPD_HAVE_ASM=@0@'.format(asm_enabled ? 1 : 0)]
cpu_baseline_c_args = []
lib_c_args += asm_c_args

# Eliminate DSP fallbacks the build target can never reach (dav1d's trim_dsp).
trim_dsp = get_option('trim_dsp') == 'true' or (
  get_option('trim_dsp') == 'if-release' and get_option('buildtype') == 'release')
trim_c_args = trim_dsp ? ['-DWPD_TRIM_DSP_FUNCTIONS=1'] : []

if asm_enabled
  if cpu_family.startswith('x86')
    # cargo's build script is what actually invokes nasm; finding it here just
    # turns a missing assembler into a configure error rather than a link one.
    nasm = find_program('nasm')
  elif cpu_family == 'aarch64'
    inc_dirs += [include_directories('src/aarch64')]
    # dotprod and i8mm, probed the way dav1d does: an assembler may accept the
    # instructions without being able to toggle the extension on and off with
    # .arch_extension, and an assembler that takes the directive still has to
    # be asked whether the instructions themselves assemble.
    aarch64_extensions = {
      'dotprod': 'sdot v0.4s, v0.16b, v0.16b',
      'i8mm': 'usdot v0.4s, v0.16b, v0.16b',
    }
    foreach name, instr : aarch64_extensions
      upper = name.to_upper()
      archext = cc.compiles(
        '__asm__ (".arch_extension @0@\\n");'.format(name),
        name: '.arch_extension ' + name)
      code = '__asm__ ('
      if archext
        code += '".arch_extension @0@\\n"'.format(name)
      endif
      code += '"@0@\\n");'.format(instr)
      lib_c_args += '-DHAVE_AS_ARCHEXT_@0@_DIRECTIVE=@1@'.format(
        upper, archext ? 1 : 0)
      lib_c_args += '-DHAVE_@0@=@1@'.format(
        upper, cc.compiles(code, name: upper) ? 1 : 0)
    endforeach

  elif cpu_family.startswith('arm')
    inc_dirs += [include_directories('src/arm')]

    armv6 = cc.compiles(
      '''#if !defined(__ARM_ARCH) || __ARM_ARCH < 6
         #error no ARMv6 on this target
         #endif
         int main(void) { return 0; }''',
      name: 'ARMv6 target')
    armv6t2 = cc.compiles(
      '''#if !defined(__ARM_ARCH_6T2__) && (!defined(__ARM_ARCH) || __ARM_ARCH < 7)
         #error no Thumb-2 on this target
         #endif
         int main(void) { return 0; }''',
      name: 'ARMv6T2 target')
    armv6t2_external = armv6t2 and cc.compiles(
      'void f(void) { __asm__(".syntax unified\\nsbfx r0, r0, #0, #1"); }',
      name: 'ARMv6T2 assembler support')

    if armv6
      cpu_baseline_c_args += ['-DWPD_ARM_ARMV6_ASM=1']
    endif
    if armv6t2
      lib_c_args += ['-DWPD_ARM_ARMV6T2_ASM=1']
    endif
    if armv6t2_external
      lib_c_args += ['-DWPD_ARM_ARMV6T2_EXTERNAL_ASM=1']
    endif
  endif
endif

lib_c_args += cpu_baseline_c_args

message('Architecture assembly: ' + (asm_enabled ? 'enabled' : 'disabled'))
message('Trim DSP functions: ' + (trim_dsp ? 'enabled' : 'disabled'))

# The library, the assembly, and whatever C has not been ported yet are built
# by cargo; Meson keeps the C test harnesses, the tools and the testdata suite.
# Cargo tracks its own inputs far better than a file list here could, so the
# target is always stale and cargo decides whether there is work to do.
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

cargo_profile = {
  'release': 'release',
  'debugoptimized': 'debugopt',
  'minsize': 'release',
  'debug': 'debug',
  'plain': 'release',
}.get(get_option('buildtype'), 'release')

# The C the port has not reached yet is compiled by cargo's build script, so
# meson's sanitizer selection has to be handed across; the cc crate reads
# CFLAGS. Rust's own sanitizer support needs nightly and is wired up in
# scripts/sanitize.sh once there is Rust code to instrument.
cargo_env = environment()
if get_option('b_sanitize') != 'none'
  cargo_env.append('CFLAGS', '-fsanitize=' + get_option('b_sanitize'),
                   '-fno-omit-frame-pointer', separator: ' ')
  # The tool is linked by rustc, so the sanitizer runtime has to reach the
  # link line there too; cargo_tool.sh turns this into RUSTFLAGS.
  cargo_env.set('WPD_SANITIZE', get_option('b_sanitize'))
endif
soversion = meson.project_version()

libwpd_artifacts = custom_target(
  'libwpd',
  output: ['libwpd.a', 'libwpd.so.' + soversion],
  command: [
    files('tools/cargo_build.sh'),
    source_root,
    meson.current_build_dir(),
    'wpd',
    cargo_profile,
    soversion,
    cargo_features,
  ],
  env: cargo_env,
  build_always_stale: true,
  build_by_default: true,
  install: true,
  install_dir: [get_option('libdir'), get_option('libdir')],
)
libwpd = libwpd_artifacts[0]

install_headers('include/wpd.h')

import('pkgconfig').generate(
  name: 'wpd',
  filebase: 'wpd',
  description: 'A fast WebP decoder',
  version: meson.project_version(),
  libraries: ['-L${libdir}', '-lwpd'],
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

# The tool is a cargo binary; it links the same archive the library target
# stages, so it goes through the C ABI exactly as an outside consumer would.
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

fuzz_test = executable(
  'test_fuzz',
  files('tests/fuzz.c'),
  dependencies: wpd_dep,
  build_by_default: false,
  install: false,
)

# A row function writing outside its row is invisible to the reference
# comparisons, so this one backs each row onto an unmapped page instead.
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

# libwebp is the reference the parity suite and libwebpdec measure against, so
# which one gets linked is a correctness question, not just a convenience: a
# released libwebp predates decoder changes wpd already matches. The wrap pins a
# revision known to agree, and is preferred over whatever the system carries.
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

checkasm_dep = dependency(
  'checkasm',
  fallback: ['checkasm', 'checkasm_dep'],
  required: false,
)

if asm_enabled and checkasm_dep.found()
  # trim_dsp lets the compiler drop every fallback the build target cannot
  # reach, which is exactly what checkasm compares the assembly against. So it
  # gets its own copy of the library with trim_dsp off; otherwise the lower
  # CPU tiers vanish and their tests silently stop running.
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
      soversion,
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

  # checkasm reaches past the public API to poke the DSP tables and override
  # the CPU flags, so it links the static archive rather than the shared one.
  checkasm = executable(
    'checkasm',
    files(
      'tests/checkasm/main.c',
      'tests/checkasm/lossless.c',
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
