# Every row of `Encoding.list`, generated from the ruby 4.0.6 oracle by
# `tools/encoding_tables.rb`. The whole list is printed because the registry
# IS the contract: the order, the names, the aliases, `dummy?` and
# `ascii_compatible?` are all reflection reads, and each one of them feeds a
# constant.

def show(label)
  puts "#{label}: #{yield.inspect}"
rescue Exception => e # rubocop:disable Lint/RescueException
  puts "#{label}: #{e.class}: #{e.message}"
end

puts "list: #{Encoding.list.size}"
Encoding.list.each do |e|
  puts [e.name, e.names.join(","), e.dummy?, e.ascii_compatible?, e.inspect].join("\t")
end
puts "name_list: #{Encoding.name_list.size}"
puts "aliases: #{Encoding.aliases.size}"
puts "constants: #{Encoding.constants.count { |c| Encoding.const_get(c).is_a?(Encoding) }}"

# The two constant spellings CRuby gives one mixed-case name, and the digit
# rule that gives `646` none of its own.
show("Windows_1250") { Encoding::Windows_1250 }
show("WINDOWS_1250") { Encoding::WINDOWS_1250 }
show("EucJP") { Encoding::EucJP }
show("EUCJP") { Encoding::EUCJP }
show("EBCDIC_CP_US") { Encoding::EBCDIC_CP_US }
show("Big5_HKSCS_2008") { Encoding::Big5_HKSCS_2008 }
show("no 646 constant") { Encoding.constants.include?(:"646") }
show("UNICODE_VERSION") { Encoding::UNICODE_VERSION }

# The runtime selectors follow `default_external`/`default_internal` rather
# than sitting in any row's alias list.
show("default_external names") { Encoding.default_external.names }
show("internal in name_list") { Encoding.name_list.include?("internal") }
show("aliases has internal") { Encoding.aliases.key?("internal") }
show("find external") { Encoding.find("external") }
show("find internal") { Encoding.find("internal") }
show("find Latin-1") { Encoding.find("Latin-1") }

# The single-byte families this runtime now maps for real.
show("ISO-8859-5 cyrillic") { "\xE0".dup.force_encoding("ISO-8859-5").encode("UTF-8") }
show("KOI8-U ghe") { "\xAD".dup.force_encoding("KOI8-U").encode("UTF-8") }
show("IBM437 box") { "\xDB".dup.force_encoding("IBM437").encode("UTF-8") }
show("CP866 be") { "\xE0".dup.force_encoding("CP866").encode("UTF-8") }
show("macRoman omega") { "\xBD".dup.force_encoding("macRoman").encode("UTF-8") }
show("TIS-620 thai") { "\xA1".dup.force_encoding("TIS-620").encode("UTF-8") }
show("Windows-874 thai") { "\xA1".dup.force_encoding("Windows-874").encode("UTF-8") }
show("Windows-1258 dong") { "\xFE".dup.force_encoding("Windows-1258").encode("UTF-8") }
show("into ISO-8859-7") { "αβ".encode("ISO-8859-7").bytes }
show("into IBM866") { "АБ".encode("IBM866").bytes }
show("ISO-8859-3 unassigned") { "\xA5".dup.force_encoding("ISO-8859-3").encode("UTF-8") }
show("ISO-8859-3 valid") { "\xA5".dup.force_encoding("ISO-8859-3").valid_encoding? }
show("ISO-8859-3 length") { "\xA5\xA6".dup.force_encoding("ISO-8859-3").length }
show("ISO-8859-9 upcase") { "\xFD".dup.force_encoding("ISO-8859-9").upcase.bytes }

# Registered-only rows answer every reflection question and carry ASCII
# through. What they cannot do is convert their own high bytes -- see
# `tests/gaps/encoding_registered_only.rb`.
show("EUC-KR ascii") { "abc".encode("EUC-KR").bytes }
show("EUC-KR ascii back") { "abc".dup.force_encoding("EUC-KR").encode("UTF-8") }
show("EUC-KR compatible?") { Encoding.compatible?("abc", "d".dup.force_encoding("EUC-KR")) }
show("EUC-KR names") { Encoding::EUC_KR.names }
show("EUC-KR dummy?") { Encoding::EUC_KR.dummy? }
show("EUC-KR ascii_compatible?") { Encoding::EUC_KR.ascii_compatible? }
