# The WHOLE encode repertoire of the two families whose WHATWG table
# disagrees with CRuby's, as a count and a checksum.
#
# zeo's multi-byte mapping comes from encoding_rs, and its Big5 maps MORE
# than CRuby (the HKSCS-adjacent region, Cyrillic included) while its EUC-JP
# is decode-only for JIS X 0212, which CRuby reaches through the SS3 plane.
# `enc/mb_encode_delta.rs` carries the difference -- 1,161 + 175 denied
# scalars and 414 + 6,072 allowed ones, generated from ruby by
# `tools/mb_encode_delta.rb`.
#
# A checksum rather than 27,000 rows: the point is that NOTHING moved, and
# a diff of one row is as much a failure as a diff of a thousand. The named
# rows below are what tells you WHICH direction moved when it does -- one
# per reason the delta exists.
#
# The DECODE direction has no delta and needs none; it already agreed.

require "digest"

%w[Big5 EUC-JP].each do |enc|
  ok = []
  # Bounded at U+2FFFF rather than U+10FFFF: neither family maps anything
  # above it, so the tail is a million iterations that answer nothing --
  # and the whole sweep runs twice, once per engine, on every suite run.
  (0..0x2FFFF).each do |cp|
    next if cp.between?(0xD800, 0xDFFF)
    s = begin
      cp.chr(Encoding::UTF_8)
    rescue RangeError
      next
    end
    begin
      ok << "#{cp}:#{s.encode(enc).unpack1('H*')}"
    rescue Encoding::UndefinedConversionError
    end
  end
  puts "#{enc}\t#{ok.size}\t#{Digest::SHA256.hexdigest(ok.join(','))[0, 16]}"
end

def show(name)
  puts "#{name}\t#{yield}"
rescue Exception => e
  puts "#{name}\t#{e.class}"
end

# Big5 DENIES what WHATWG maps.
show("big5 cyrillic") { "Ж".encode("Big5").unpack1("H*") }
# Big5 ALLOWS what WHATWG does not.
show("big5 ok") { "中".encode("Big5").unpack1("H*") }
# EUC-JP reaches JIS X 0212 through SS3.
show("euc jis0212") { "é".encode("EUC-JP").unpack1("H*") }
show("euc roundtrip") { "é".encode("EUC-JP").encode("UTF-8") == "é" }
show("euc kana") { "あ".encode("EUC-JP").unpack1("H*") }
show("euc ascii") { "a".encode("EUC-JP").unpack1("H*") }

# DECODE is untouched, in both families.
show("big5 decode") { "\xa4\xa4".dup.force_encoding("Big5").encode("UTF-8") }
show("euc decode") { "\xa4\xa2".dup.force_encoding("EUC-JP").encode("UTF-8") }
show("euc decode ss3") { "\x8f\xab\xb1".dup.force_encoding("EUC-JP").encode("UTF-8") }
__END__
Big5	14029	5e276003a8f8676f
EUC-JP	13137	9be5fe3c47147a0e
big5 cyrillic	Encoding::UndefinedConversionError
big5 ok	a4a4
euc jis0212	8fabb1
euc roundtrip	true
euc kana	a4a2
euc ascii	61
big5 decode	中
euc decode	あ
euc decode ss3	é
