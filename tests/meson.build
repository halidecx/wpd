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
#
# The huffman_* stills are hand-written by wpd-test-data/scripts/mk_huffman_codes.py
# for code shapes no encoder emits; their hashes come from the pixels the
# specification says they decode to, not from a decode. huffman_simple_duplicate
# and huffman_simple_single differ in bytes and must share a hash.
testdata = [
  ['a_lossy',             'Argb',     'da3731b078d166fdc013b5224fdf0a08'],
  ['a_lossy',             'bgr',      '3b6907ba6cdb7ddb49ea94d3e04a3723'],
  ['a_lossy',             'bgra',     '5156051fb7abc6242e8c1bb7b7da6505'],
  ['a_lossy',             'bgrA',     'fd5cd2fda9cd4a63e34716b4afa9b64c'],
  ['a_lossy',             'rgb',      '131775b2e84b04130578ca49d738e34b'],
  ['a_lossy',             'rgbA',     '35a3d0e0b8ca62bcef4d3e71e68cab31'],
  ['a_lossy',             'rgba',     'f33487c7a52c0f6798a5408b234acaaa'],
  ['a_lossy',             'argb',     '0ceddbf38163256bc45bb5e7adbf3f0c'],
  ['a_lossy',             'yuv420p',  'e30670f89a1bd43b9593e8ad5f7567ba'],
  ['a_lossy',             'yuva420p', '6aba582a8c4eee341196fcfc7229ff3a'],
  ['alpha_uncompressed_gradient', 'argb', 'ff31569bf9f4921691b644d4438b7045'],
  ['alpha_uncompressed_horizontal', 'argb', 'ff31569bf9f4921691b644d4438b7045'],
  ['alpha_uncompressed_none', 'argb', 'ff31569bf9f4921691b644d4438b7045'],
  ['alpha_uncompressed_vertical', 'argb', 'ff31569bf9f4921691b644d4438b7045'],
  # A palette ALPH with a colour cache, and one with no palette transform at
  # all: the two shapes the eight-bit alpha decode hands back to the ARGB
  # canvas, and which nothing else here reaches.
  ['a_lossy_cached',      'Argb',     '925268fc7bf4bcc86010b276eb48d564'],
  ['a_lossy_cached',      'bgr',      '48a92b37dd942ee8d0f6868bce5ed7d9'],
  ['a_lossy_cached',      'bgra',     '428f8991b0ddbfc09437c7d3e897eb5f'],
  ['a_lossy_cached',      'bgrA',     'e42f88593edc3a641a7c12e063dcf14b'],
  ['a_lossy_cached',      'rgb',      '9c4377bb79e1e5ce7fbb3d45f397211c'],
  ['a_lossy_cached',      'rgbA',     '8ade400c6cffe162213d9ef3930f7033'],
  ['a_lossy_cached',      'rgba',     'f06641852acdad4681a1ddbf8b884286'],
  ['a_lossy_cached',      'argb',     'ceceeec297bc8a95fc0a2061e85b3e79'],
  ['a_lossy_cached',      'yuv420p',  '2e2f41031f23df1ebd53c44338552153'],
  ['a_lossy_cached',      'yuva420p', 'c8e93df5f0e57974b084feac387a727f'],
  ['a_lossy_gradient',    'Argb',     '57b65a5ab85469f74ab2ff2fdbb0ca01'],
  ['a_lossy_gradient',    'bgr',      '7c762e2d7a696a50b65b7b4a24207ff8'],
  ['a_lossy_gradient',    'bgra',     '6296c737d5d9f944d5660224139f0898'],
  ['a_lossy_gradient',    'bgrA',     'b2d5ccbc893e51084e764f05b961e63c'],
  ['a_lossy_gradient',    'rgb',      '20316b606e7ef5672544b48c5a50692d'],
  ['a_lossy_gradient',    'rgbA',     '56d80e915554b248f58188738358bbad'],
  ['a_lossy_gradient',    'rgba',     '4b473d98c8e8ea1cf7f7758c3c977332'],
  ['a_lossy_gradient',    'argb',     '0d4eac4f29fea5d909e139b2450e2822'],
  ['a_lossy_gradient',    'yuv420p',  '337e817f7ff9fc8be814bfc6e1bbf631'],
  ['a_lossy_gradient',    'yuva420p', '296024dddd67eb94f19104d9f5ed6532'],
  ['anim_rgb',            'argb',     '15b10478898dcbf1cf6c8acfa7f061dc'],
  ['anim_yuv',            'argb',     '997bd7e4f5a1a0d1828f461b73d725fa'],
  ['anim_yuv',            'yuv420p',  'b270ef1522a873dd5907a3fa915cfcfe'],
  ['anim_yuv',            'yuva420p', 'dceef9e28b7ef3096d7619c45bdbcf40'],
  ['anim_yuva',           'argb',     '422a19c12d1e9f94bdebad9d0e0ef3f2'],
  ['anim_yuva',           'yuv420p',  '10ffb94d93152b7e8b197348b90a4f76'],
  ['anim_yuva',           'yuva420p', 'd45f856333c1e6a04211226deee93a6d'],
  ['dispose_bg_blend',    'argb',     '46b86f43959e2c2bd39677fb45d65a79'],
  ['dispose_bg_fullframe', 'argb',    'c4cc62cefee35718a1c333dc3a269211'],
  ['dispose_bg_noblend',  'argb',     '46b86f43959e2c2bd39677fb45d65a79'],
  ['dispose_none_blend',  'argb',     'fab11af4c45b6ea476e53e659cae61e6'],
  ['dispose_none_noblend', 'argb',    'b7d6d6d54172a4b01c1cca6d6f864691'],
  ['durations',           'argb',     '1d9390b1f3bfde14a23f97a6146830c5'],
  ['edge_frames',         'argb',     'd7dd013e4500b3e8280bb90d33f38b62'],
  ['huffman_long_codes',  'argb',     '7d06b583e8e6f0d6cdb13f7727919a48'],
  ['huffman_simple_duplicate', 'argb', 'b99bb4f17cfde05793a3688013456027'],
  ['huffman_simple_forms', 'argb',    'dc53075dd99b2220b29c257f8b487b2f'],
  ['huffman_simple_single', 'argb',   'b99bb4f17cfde05793a3688013456027'],
  ['keyframe_midstream',  'argb',     '43e119faef85a8fc4303ec01e7d30327'],
  ['kitchen_sink',        'argb',     '42ba8f19e026465ce359f10cbf150ce6'],
  ['lossless',            'argb',     'be49864598ae0fef693581929493ba50'],
  ['lossy',               'argb',     '39e0c15e2668ba4903b63746042c6377'],
  ['lossy',               'yuv420p',  '1df746b2554d9b39013ee46cfe1e8a6a'],
  ['mixed_codecs',        'Argb',     'a9e9e69d5cf5151a472432681d183ad2'],
  ['mixed_codecs',        'bgr',      '4dd4bc6b2254feb2e383f131e286957d'],
  ['mixed_codecs',        'bgra',     '45167018e4a646432b77bd42c1512b8a'],
  ['mixed_codecs',        'bgrA',     'd0316b09c19d663e42e6e7652d1b9106'],
  ['mixed_codecs',        'rgb',      '8ba7d2105ba47c5380d23a9e8ce00094'],
  ['mixed_codecs',        'rgbA',     '4b6d7029a1f5e58ca58b27b25672c616'],
  ['mixed_codecs',        'rgba',     'cae6cdf165461a26b7993349c8b3cbff'],
  ['mixed_codecs',        'argb',     '00e1457ebd70181ffa3fe06352763e32'],
  ['mixed_codecs',        'auto',     '23f4a3fd9fe7491312adce6ce7ee063c'],
  ['odd_a_lossy',         'Argb',     '7a937288af1db5d8444986dd225a3327'],
  ['odd_a_lossy',         'bgr',      'd431a072b1063a77563d981afdb5e688'],
  ['odd_a_lossy',         'bgra',     '088456b543c0c068d0077360c3f67014'],
  ['odd_a_lossy',         'bgrA',     'd9a191a083bd06a318ec3c4060386a9b'],
  ['odd_a_lossy',         'rgb',      'afea221f592a400663a4b78db0edc3c6'],
  ['odd_a_lossy',         'rgbA',     '8d176d00d846a142372b77ea3cc17603'],
  ['odd_a_lossy',         'rgba',     'e4c6af071a2de8d2a4e354a5343500df'],
  ['odd_a_lossy',         'argb',     '8d81bad7067cf823e97ecefbb91933fd'],
  ['odd_a_lossy',         'yuv420p',  '05a447c88b226abd322b350616c1a4b6'],
  ['odd_a_lossy',         'yuva420p', 'e38e4805f28e55afdc7df404f863a057'],
  ['odd_canvas',          'argb',     '2ec8c6463f44125d04564413cbf9efd5'],
  ['odd_frames',          'argb',     '757df6c49be06a9948a11a35b1ec2cbf'],
  ['odd_lossy',           'Argb',     'a0fa1bff5422a752e7f561692d83cd2a'],
  ['odd_lossy',           'bgr',      'f6adb730cf29bb13c99d34d070d2d51d'],
  ['odd_lossy',           'bgra',     'bc802ba2087a44c29ac9bf3589b72890'],
  ['odd_lossy',           'bgrA',     'bc802ba2087a44c29ac9bf3589b72890'],
  ['odd_lossy',           'rgb',      '44cdf40c2301c18214e8c1ea109d8b5c'],
  ['odd_lossy',           'rgbA',     '12f56e57c9b5952a584c02d29c6b3033'],
  ['odd_lossy',           'rgba',     '12f56e57c9b5952a584c02d29c6b3033'],
  ['odd_lossy',           'argb',     'a0fa1bff5422a752e7f561692d83cd2a'],
  ['odd_lossy',           'yuv420p',  'fa89834f928f1c384da3372b042e1b6c'],
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
  ['predict_topright',    'argb',     '3230e15952c4cd1950c2145b1fbbdb8f'],
  ['simplelf-lossy',      'argb',     '0b4304a51219d871cdc67e6e43c63d13'],
  ['simplelf-lossy',      'yuv420p',  '902f82e8c7df48a81824df5cc944d815'],
  ['transforms_before_palette', 'argb', '99deeacd41a60584c9f93aa4a6dd1f03'],
  ['transparent_over',    'Argb',     'b76238e7781ef9f04bff376b2c4060da'],
  ['transparent_over',    'bgr',      '091bcafcbd34c25b011dd575f5daf2ee'],
  ['transparent_over',    'bgra',     '8e90439c51b61ebd3246c8c43b980d55'],
  ['transparent_over',    'bgrA',     '35f9df56599d94ea20677753764d2c89'],
  ['transparent_over',    'rgb',      'bf1965fa7853809ac562245816a40f66'],
  ['transparent_over',    'rgbA',     '0a10bb10bbc485d9d951e516f3198c59'],
  ['transparent_over',    'rgba',     'cad61432746152ca6548c38e537f74da'],
  ['transparent_over',    'argb',     'e4aed005e206bc6b220e24210af98aa6'],
]

test('api-testdata', api_test, suite: ['testdata', 'api'], args: [testdata_dir])
test('parity', parity_test, suite: ['testdata', 'parity'], args: [testdata_dir],
     timeout: 300)
test('stream-reopen', wpd, suite: ['testdata', 'testdata-stream'],
     args: ['--info', '--stream', '1021', '--loops', '2',
            testdata_dir / 'anim_rgb.webp'])
test('rewind-replay', wpd, suite: ['testdata', 'testdata-stream'],
     args: ['--info', '--loops', '2', testdata_dir / 'anim_rgb.webp'])
test('rewind-replay-subframe', wpd, suite: ['testdata', 'testdata-stream'],
     args: ['--info', '--subframe', '--loops', '2',
            testdata_dir / 'anim_rgb.webp'])
foreach t : testdata
  input = testdata_dir / t[0] + '.webp'
  if not fs.exists(input)
    error('missing test data file ' + input)
  endif
  test('@0@-@1@'.format(t[0], t[1]), wpd,
       suite: ['testdata', 'testdata-' + t[1]],
       args: ['--verify', t[2], '-f', t[1], input])
  # The same decode driven incrementally must be bit-identical. The chunk size
  # is deliberately not a power of two, so chunk boundaries land inside chunk
  # headers and payloads rather than lining up with them.
  test('@0@-@1@-stream'.format(t[0], t[1]), wpd,
       suite: ['testdata', 'testdata-stream'],
       args: ['--verify', t[2], '-f', t[1], '--stream', '1021', input])
endforeach
