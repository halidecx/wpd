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

# The dispose_*, overlap_*, and other single-purpose animations cover the
# compositing matrix and are verified bit-exact against libwebp by
# scripts/animcheck.sh.
testdata = [
  ['a_lossy',             'yuv420p',  'e30670f89a1bd43b9593e8ad5f7567ba'],
  ['a_lossy',             'yuva420p', '6aba582a8c4eee341196fcfc7229ff3a'],
  ['anim_rgb',            'argb',     '15b10478898dcbf1cf6c8acfa7f061dc'],
  ['anim_yuv',            'yuv420p',  'b270ef1522a873dd5907a3fa915cfcfe'],
  ['anim_yuv',            'yuva420p', 'dceef9e28b7ef3096d7619c45bdbcf40'],
  ['anim_yuva',           'yuv420p',  '10ffb94d93152b7e8b197348b90a4f76'],
  ['anim_yuva',           'yuva420p', 'd45f856333c1e6a04211226deee93a6d'],
  ['dispose_bg_blend',    'argb',     '46b86f43959e2c2bd39677fb45d65a79'],
  ['dispose_bg_fullframe', 'argb',    'c4cc62cefee35718a1c333dc3a269211'],
  ['dispose_bg_noblend',  'argb',     '46b86f43959e2c2bd39677fb45d65a79'],
  ['dispose_none_blend',  'argb',     'fab11af4c45b6ea476e53e659cae61e6'],
  ['dispose_none_noblend', 'argb',    'b7d6d6d54172a4b01c1cca6d6f864691'],
  ['durations',           'argb',     '1d9390b1f3bfde14a23f97a6146830c5'],
  ['edge_frames',         'argb',     'd7dd013e4500b3e8280bb90d33f38b62'],
  ['keyframe_midstream',  'argb',     '43e119faef85a8fc4303ec01e7d30327'],
  ['kitchen_sink',        'argb',     '42ba8f19e026465ce359f10cbf150ce6'],
  ['lossless',            'argb',     'be49864598ae0fef693581929493ba50'],
  ['lossy',               'yuv420p',  '1df746b2554d9b39013ee46cfe1e8a6a'],
  ['odd_canvas',          'argb',     '2ec8c6463f44125d04564413cbf9efd5'],
  ['odd_frames',          'argb',     '757df6c49be06a9948a11a35b1ec2cbf'],
  ['overlap_bottom',      'argb',     '13629354bec1adb694dd7642bbf63e24'],
  ['overlap_contains',    'argb',     '2c6fc5f3d5183fe41c63a6098f3a5c06'],
  ['overlap_corner',      'argb',     '8351549b64b38e06ed23e0a9dce03e04'],
  ['overlap_disjoint',    'argb',     '9b71024d1a12397c3f2fc9fb8968c5d2'],
  ['overlap_exact',       'argb',     '467a108d02c2ec307229f7a9c46eb5f6'],
  ['overlap_inside',      'argb',     '42ed21a8ee6fc346f867034613e89479'],
  ['overlap_left',        'argb',     'a479da794ca91deaeb07afda8b0ae9dc'],
  ['overlap_odd',         'argb',     'a0f33aca7b8ca25c2392bae067691e41'],
  ['overlap_right',       'argb',     'f37aa4f428540bce480fd02932ff90d4'],
  ['overlap_single',      'argb',     '43c282d90fca70401385df2e928005e5'],
  ['overlap_top',         'argb',     'c623f1c3f8f3e21b8934300c8e1e9081'],
  ['palette2bpp_rgb',     'argb',     '3573fb55c12dd909d0031a061abe60ea'],
  ['palette4bpp_rgb',     'argb',     'a2947b69d2cf6ff6bce54039588259c3'],
  ['palette_rgb',         'argb',     'c8787aa91807ccd5aeddaa20e5c1385c'],
  ['simplelf-lossy',      'yuv420p',  '902f82e8c7df48a81824df5cc944d815'],
  ['transparent_over',    'argb',     'e4aed005e206bc6b220e24210af98aa6'],
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
