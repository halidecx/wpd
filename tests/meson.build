if not get_option('testdata_tests')
  subdir_done()
endif

fs = import('fs')

testdata_dir = get_option('testdata_dir')
if testdata_dir == ''
  testdata_dir = source_root / 'wpd-test-data'
endif
if not fs.is_dir(testdata_dir)
  error('testdata_tests requires a wpd-test-data directory, not found at ' +
        testdata_dir)
endif

testdata = [
  ['a_lossy',         'yuv420p',  'e30670f89a1bd43b9593e8ad5f7567ba'],
  ['a_lossy',         'yuva420p', '6aba582a8c4eee341196fcfc7229ff3a'],
  ['anim_rgb',        'argb',     '15b10478898dcbf1cf6c8acfa7f061dc'],
  ['anim_yuv',        'yuv420p',  'b270ef1522a873dd5907a3fa915cfcfe'],
  ['anim_yuv',        'yuva420p', 'dceef9e28b7ef3096d7619c45bdbcf40'],
  ['anim_yuva',       'yuv420p',  '1bcb445e1b609186da573df85ea7ccb6'],
  ['anim_yuva',       'yuva420p', '49a4d50acaa4d3225b4495c00203c166'],
  ['lossless',        'argb',     'be49864598ae0fef693581929493ba50'],
  ['lossy',           'yuv420p',  '1df746b2554d9b39013ee46cfe1e8a6a'],
  ['palette2bpp_rgb', 'argb',     '3573fb55c12dd909d0031a061abe60ea'],
  ['palette4bpp_rgb', 'argb',     'a2947b69d2cf6ff6bce54039588259c3'],
  ['palette_rgb',     'argb',     'c8787aa91807ccd5aeddaa20e5c1385c'],
  ['simplelf-lossy',  'yuv420p',  '902f82e8c7df48a81824df5cc944d815'],
]

foreach t : testdata
  input = testdata_dir / t[0] + '.webp'
  if not fs.exists(input)
    error('missing test data file ' + input)
  endif
  test('@0@-@1@'.format(t[0], t[1]), wpd,
       suite: ['testdata', 'testdata-' + t[1]],
       args: ['--verify', t[2], '-f', t[1], input])
endforeach
