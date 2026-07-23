project(
  'wpd',
  'c',
  version: '0.0.0',
  meson_version: '>=1.11.0',
  default_options: [
    'buildtype=release',
    'c_std=c11',
    'warning_level=2',
  ],
)

cc = meson.get_compiler('c')
source_root = meson.current_source_dir()
cpu_family = host_machine.cpu_family()

inc_dirs = [include_directories('include', 'src')]
lib_sources = files(
  'src/ffvp8.c',
  'src/vp8.c',
  'src/vp8dsp.c',
  'src/vp56rac.c',
  'src/vp8pred.c',
  'src/compat.c',
)
lib_objects = []
lib_c_args = [
  '-Wno-unused-parameter',
  '-Wno-maybe-uninitialized',
  '-Wno-parentheses',
]

asm_enabled = get_option('enable_asm') and (
  cpu_family == 'aarch64' or
  cpu_family.startswith('arm') or
  cpu_family.startswith('x86')
)

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
        'src/x86/vp8dsp.asm',
        'src/x86/vp8_intrapred.asm',
        'src/x86/videodsp.asm',
      ),
      extra_args: ['-DHAVE_AVX2_EXTERNAL=1'],
    )
    lib_objects += nasm_gen.process(files('src/x86/vp8dsp_loopfilter.asm'))
    lib_sources += files(
      'src/x86/vp8dsp_init.c',
      'src/x86/vp8pred_init.c',
      'src/x86/videodsp_init.c',
    )
    lib_c_args += [
      '-DFFVP8_ENABLE_X86_SIMD=1',
      '-DFFVP8_PRED_X86=1',
      '-DHAVE_YASM=1',
      '-DHAVE_AVX2_EXTERNAL=1',
      '-DARCH_X86_32=@0@'.format(is_x86_64 ? 0 : 1),
    ]
  elif cpu_family == 'aarch64'
    inc_dirs += [include_directories('src/aarch64')]
    lib_sources += files(
      'src/aarch64/vp8dsp_neon.S',
      'src/aarch64/vp8dsp_init_aarch64.c',
      'src/aarch64/vp8pred_neon.S',
      'src/aarch64/vp8pred_init.c',
      'src/aarch64/videodsp.S',
      'src/aarch64/videodsp_init.c',
    )
    lib_c_args += ['-DFFVP8_PRED_AARCH64=1']
  elif cpu_family.startswith('arm')
    inc_dirs += [include_directories('src/arm')]
    lib_sources += files(
      'src/arm/vp8_armv6.S',
      'src/arm/vp8dsp_armv6.S',
      'src/arm/vp8dsp_neon.S',
      'src/arm/vp8dsp_init_arm.c',
      'src/arm/vp8dsp_init_armv6.c',
      'src/arm/vp8dsp_init_neon.c',
      'src/arm/vp8pred_neon.S',
      'src/arm/vp8pred_init.c',
      'src/arm/videodsp_armv5te.S',
      'src/arm/videodsp_init.c',
    )
    lib_c_args += [
      '-DFFVP8_ARM_NEON_ASM=1',
      '-DFFVP8_PRED_ARM=1',
    ]
  endif
endif

message('Architecture assembly: ' + (asm_enabled ? 'enabled' : 'disabled'))

libffvp8 = static_library(
  'ffvp8',
  lib_sources,
  lib_objects,
  include_directories: inc_dirs,
  c_args: lib_c_args,
  install: false,
)

ffvp8_dep = declare_dependency(
  link_with: libffvp8,
  include_directories: inc_dirs,
)

executable(
  'wpd',
  'tools/wpd.c',
  dependencies: ffvp8_dep,
  install: false,
)

checkasm_dep = dependency(
  'checkasm',
  fallback: ['checkasm', 'checkasm_dep'],
  required: false,
)

if asm_enabled and checkasm_dep.found()
  checkasm = executable(
    'checkasm',
    files(
      'tests/checkasm/main.c',
      'tests/checkasm/vp8dsp.c',
      'tests/checkasm/vp8pred.c',
      'tests/checkasm/videodsp.c',
    ),
    dependencies: [ffvp8_dep, checkasm_dep],
    include_directories: include_directories('tests/checkasm'),
    c_args: lib_c_args,
    build_by_default: false,
    install: false,
  )
  test('checkasm', checkasm, suite: 'checkasm', timeout: 180)
endif
