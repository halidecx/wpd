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

vcs_version = vcs_tag(
  command: ['git', '-C', source_root, 'rev-parse', '--short=7', 'HEAD'],
  fallback: 'unknown',
  input: 'tools/vcs_version.h.in',
  output: 'vcs_version.h',
)

getopt_sources = []
getopt_inc_dirs = []
if not cc.has_function('getopt_long', prefix: '#include <getopt.h>')
  getopt_sources = files('tools/compat/getopt.c')
  getopt_inc_dirs = include_directories('include/compat')
endif

inc_dirs = [include_directories('.', 'include', 'src')]
lib_sources = files(
  'src/wpd_decoder.c',
  'src/vp8.c',
  'src/vp8dsp.c',
  'src/vp56rac.c',
  'src/vp8pred.c',
  'src/vp8l_dsp.c',
  'src/yuvdsp.c',
  'src/rescaler.c',
  'src/wpd_compat.c',
  'src/cpu.c',
)
lib_objects = []
lib_c_args = ['-Wno-unused-parameter', '-DWPD_BUILDING']

lib_c_args += '-DWPD_HAVE_GETAUXVAL=@0@'.format(
  cc.has_function('getauxval', prefix: '#include <sys/auxv.h>') ? 1 : 0)
lib_c_args += '-DWPD_HAVE_ELF_AUX_INFO=@0@'.format(
  cc.has_function('elf_aux_info', prefix: '#include <sys/auxv.h>') ? 1 : 0)

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
    nasm = find_program('nasm')
    is_x86_64 = cpu_family == 'x86_64'
    if host_machine.system() == 'windows'
      nasm_format = 'win'
    elif host_machine.system() == 'darwin'
      nasm_format = 'macho'
    else
      nasm_format = 'elf'
    endif
    nasm_format += is_x86_64 ? '64' : '32'

    nasm_prefix_args = []
    if host_machine.system() in ['darwin', 'ios', 'tvos', 'os/2'] or (
       host_machine.system() == 'windows' and not is_x86_64)
      nasm_prefix_args = ['-DPREFIX=1']
    endif

    nasm_gen = generator(
      nasm,
      output: '@BASENAME@.obj',
      depfile: '@BASENAME@.obj.ndep',
      arguments: [
        '-w-label-orphan',
        '-w-implicit-abs-deprecated',
        '-f', nasm_format,
        '-DPIC=1',
        '-DHAVE_ALIGNED_STACK=1',
        '-DHAVE_X86_SSE2AVX=0',
        '-DARCH_X86_64=@0@'.format(is_x86_64 ? 1 : 0),
        '-DARCH_X86_32=@0@'.format(is_x86_64 ? 0 : 1),
        nasm_prefix_args,
        '-I', source_root / 'src' + '/',
        '-MQ', '@OUTPUT@',
        '-MD', '@DEPFILE@',
        '@EXTRA_ARGS@',
        '@INPUT@',
        '-o', '@OUTPUT@',
      ],
    )

    lib_objects += nasm_gen.process(
      files(
        'src/x86/vp8l.asm',
        'src/x86/vp8dsp.asm',
        'src/x86/vp8_intrapred.asm',
      ),
      extra_args: ['-DHAVE_AVX2_EXTERNAL=1'],
    )
    lib_objects += nasm_gen.process(files(
      'src/x86/vp8dsp_loopfilter.asm',
      'src/x86/yuvdsp.asm',
    ))
    lib_sources += files(
      'src/x86/cpu.c',
      'src/x86/wpd_simd_constants.c',
    )
  elif cpu_family == 'aarch64'
    inc_dirs += [include_directories('src/aarch64')]
    lib_sources += files(
      'src/arm/cpu.c',
      'src/aarch64/vp8l_neon.S',
      'src/aarch64/vp8dsp_neon.S',
      'src/aarch64/vp8pred_neon.S',
      'src/aarch64/yuvdsp_neon.S',
    )
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

    lib_sources += files(
      'src/arm/cpu.c',
      'src/arm/vp8dsp_neon.S',
      'src/arm/vp8pred_neon.S',
    )
    if armv6
      lib_sources += files(
        'src/arm/vp8dsp_armv6.S',
      )
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

libwpd = library(
  'wpd',
  lib_sources,
  lib_objects,
  include_directories: inc_dirs,
  c_args: lib_c_args + trim_c_args,
  version: meson.project_version(),
  soversion: meson.project_version().split('.')[0],
  gnu_symbol_visibility: 'hidden',
  install: true,
)

install_headers('include/wpd.h')

import('pkgconfig').generate(
  libwpd,
  name: 'wpd',
  filebase: 'wpd',
  description: 'A fast WebP decoder',
)

# The bundled tools reach past the public API (cpu flag overrides), which the
# shared library hides, so link them against the static objects instead.
if get_option('default_library') == 'both'
  libwpd_internal = libwpd.get_static_lib()
elif get_option('default_library') == 'static'
  libwpd_internal = libwpd
else
  libwpd_internal = static_library(
    'wpd_internal',
    lib_sources,
    lib_objects,
    include_directories: inc_dirs,
    c_args: lib_c_args + trim_c_args,
    build_by_default: false,
    install: false,
  )
endif

wpd_dep = declare_dependency(
  link_with: libwpd_internal,
  include_directories: inc_dirs,
  compile_args: asm_c_args + cpu_baseline_c_args + trim_c_args + ['-DWPD_STATIC'],
)

wpd = executable(
  'wpd',
  files(
    'tools/md5.c',
    'tools/wpd.c',
  ),
  vcs_version,
  getopt_sources,
  dependencies: wpd_dep,
  include_directories: getopt_inc_dirs,
  install: false,
)

md5_test = executable(
  'test_md5',
  files(
    'tests/md5.c',
    'tools/md5.c',
  ),
  include_directories: include_directories('tools'),
  install: false,
)
test('md5', md5_test, suite: 'md5')

# libwebp is the reference wpd is measured against, so which one gets linked
# is a correctness question, not just a convenience: a released libwebp
# predates decoder changes wpd already matches. The wrap pins a revision known
# to agree, and is preferred over whatever the system carries.
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
    # webpdemux pulls in the full webp library, which already decodes; adding
    # webpdecoder alongside it would only offer the linker a second copy.
    libwebpdec_deps = [libwebp_subproject.dependency('webpdemux')]
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
  libwpd_checkasm = static_library(
    'wpd_checkasm',
    lib_sources,
    lib_objects,
    include_directories: inc_dirs,
    c_args: lib_c_args + ['-DWPD_CHECKASM=1'],
    build_by_default: false,
    install: false,
  )

  wpd_checkasm_dep = declare_dependency(
    link_with: libwpd_checkasm,
    include_directories: inc_dirs,
  )

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
    c_args: lib_c_args + ['-DWPD_CHECKASM=1'],
    build_by_default: true,
    install: false,
  )
  test('checkasm', checkasm, suite: 'checkasm', timeout: 180)
  benchmark('checkasm', checkasm, suite: 'checkasm', timeout: 3600,
            args: '--bench')
endif

subdir('tests')
