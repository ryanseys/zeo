# kconv -- the Kconv wrapper over NKF (vendored upstream lib/kconv.rb) and
# its String patches (#tojis/#toeuc/#tosjis/#toutf8/#is*).
require "kconv"

p defined?(Kconv)
p Kconv::UTF8
p "テスト".tojis.unpack1("H*")
p "テスト".tosjis == "テスト".encode("Shift_JIS")
p "テスト".toeuc == "テスト".encode("EUC-JP")
p "テスト".encode("Shift_JIS").b.toutf8
p ["テスト".toutf16.encoding, "テスト".toutf16.unpack1("H*")]
p Kconv.guess("テスト".encode("EUC-JP").b)
p Kconv.kconv("テスト".encode("EUC-JP").b, Kconv::UTF8)
p Kconv.tojis("テスト") == "テスト".tojis
p "abc".isjis
p "\e$B%F%9%H\e(B".isjis
p "テスト".isutf8
p "テスト".encode("Shift_JIS").b.iseuc
p "テスト".encode("EUC-JP").b.iseuc
