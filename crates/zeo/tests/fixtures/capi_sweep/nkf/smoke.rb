require "nkf"

p NKF.nkf("-w", "\x82\xa0\x82\xa2".b).bytes
p NKF.nkf("-s", "あい").bytes
p NKF.nkf("-j", "あ").bytes
puts NKF.guess("abc"), NKF.guess("あ".encode("Shift_JIS").b)
puts NKF.nkf("-Z1 -w", "ＡＢＣ"), NKF.nkf("-w -x", "ｱ")
