# The multibyte mapping tables landed, so the rows below convert exactly as
# CRuby does: EUC-KR, GB18030, GB2312, CP949, Big5-HKSCS, UTF8-MAC, eucJP-ms
# and CP51932 all round-trip here.
#
# What still answers `Encoding::ConverterNotFoundError` answers it under ruby
# TOO, which is why this is a pinned agreement and not a gap: EUC-TW, and the
# single-byte rows CRuby itself registers with no transcoder of its own
# (`Windows-1258`, `macThai` and their two siblings).
#
# The point of the file is that a registered-only row stays complete in every
# OTHER way -- `#length`, `#valid_encoding?` and the reflection surface answer
# whatever ruby answers. See `EncKind::Registered`.

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
