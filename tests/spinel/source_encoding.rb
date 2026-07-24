# Issue #843. `__ENCODING__` should behave like an Encoding value,
# not a plain String.

puts __ENCODING__.class
puts __ENCODING__.name
puts __ENCODING__.to_s
puts __ENCODING__.is_a?(Encoding)
puts __ENCODING__ == __ENCODING__
puts __ENCODING__ != "UTF-8"

values = [__ENCODING__, 1]
puts values[0]

enc_hash = {}
enc_hash[__ENCODING__] = 7
puts enc_hash[__ENCODING__]

mixed = ARGV.length > 0 ? __ENCODING__ : 1
puts mixed
