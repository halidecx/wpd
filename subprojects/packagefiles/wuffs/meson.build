# Wuffs ships as one amalgamated C file that the including translation unit
# compiles with WUFFS_IMPLEMENTATION, so all this exposes is where it is.
project('wuffs', 'c')

# The unsupported snapshot reports version 0.0.0, so the banner names the
# pinned commit instead.
revision = run_command(
  'git', '-C', meson.current_source_dir(), 'rev-parse', '--short=12', 'HEAD',
  check: false,
)

wuffs_dep = declare_dependency(
  include_directories: include_directories('release/c'),
  compile_args: '-DWUFFSDEC_REVISION="@0@"'.format(
    revision.returncode() == 0 ? revision.stdout().strip() : 'unknown'),
)
