# 31 of the 103 encodings are registered by NAME only: they answer every
# reflection question (`Encoding.list`, `#names`, `#dummy?`,
# `#ascii_compatible?`, their constants) and carry ASCII through, but this
# runtime holds no byte<->character mapping for them, so converting a high
# byte raises `Encoding::ConverterNotFoundError` where CRuby converts.
#
# They are the multibyte families whose mapping tables are not in the tree
# (EUC-KR, EUC-TW, GB18030, GB2312, the Big5 variants, the emoji vendor
# pages, ...), the four single-byte rows CRuby itself registers with no
# transcoder, and the stateful JIS dummies beyond ISO-2022-JP. See
# `EncKind::Registered`; the fix shape is a mapping table per family, the
# same shape `tools/encoding_tables.rb` already generates for the single-byte
# rows.

def show(label)
  puts "#{label}: #{yield.inspect}"
rescue Exception => e # rubocop:disable Lint/RescueException
  puts "#{label}: #{e.class}: #{e.message}"
end

show("EUC-KR encode") { "가".encode("EUC-KR").bytes }
show("EUC-KR decode") { "\xB0\xA1".dup.force_encoding("EUC-KR").encode("UTF-8") }
show("GB18030 decode") { "\xB0\xA1".dup.force_encoding("GB18030").encode("UTF-8") }
show("GB2312 decode") { "\xB0\xA1".dup.force_encoding("GB2312").encode("UTF-8") }
show("EUC-TW decode") { "\xC4\xE9".dup.force_encoding("EUC-TW").encode("UTF-8") }
show("CP949 decode") { "\xB0\xA1".dup.force_encoding("CP949").encode("UTF-8") }
show("Big5-HKSCS decode") { "\xA4\x40".dup.force_encoding("Big5-HKSCS").encode("UTF-8") }
show("UTF8-MAC decode") { "\x65\xCC\x81".dup.force_encoding("UTF8-MAC").encode("UTF-8") }
show("eucJP-ms decode") { "\xA4\xA2".dup.force_encoding("eucJP-ms").encode("UTF-8") }
show("CP51932 decode") { "\xA4\xA2".dup.force_encoding("CP51932").encode("UTF-8") }

# The four single-byte rows CRuby registers with no transcoder of its own
# raise the SAME class there, which is why they are registered-only here too.
show("Windows-1258 decode") { "\xFE".dup.force_encoding("Windows-1258").encode("UTF-8") }
show("macThai decode") { "\xA1".dup.force_encoding("macThai").encode("UTF-8") }

# Reflection is unaffected -- these rows are complete in every other way.
show("EUC-KR length") { "\xB0\xA1".dup.force_encoding("EUC-KR").length }
show("EUC-TW valid?") { "\xC4\xE9".dup.force_encoding("EUC-TW").valid_encoding? }
