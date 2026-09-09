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
# `test/core/encoding/encoding_registered_only.rb`.
show("EUC-KR ascii") { "abc".encode("EUC-KR").bytes }
show("EUC-KR ascii back") { "abc".dup.force_encoding("EUC-KR").encode("UTF-8") }
show("EUC-KR compatible?") { Encoding.compatible?("abc", "d".dup.force_encoding("EUC-KR")) }
show("EUC-KR names") { Encoding::EUC_KR.names }
show("EUC-KR dummy?") { Encoding::EUC_KR.dummy? }
show("EUC-KR ascii_compatible?") { Encoding::EUC_KR.ascii_compatible? }
__END__
list: 103
ASCII-8BIT	ASCII-8BIT,BINARY	false	true	#<Encoding:BINARY (ASCII-8BIT)>
UTF-8	UTF-8,CP65001,locale,external,filesystem	false	true	#<Encoding:UTF-8>
US-ASCII	US-ASCII,ASCII,ANSI_X3.4-1968,646	false	true	#<Encoding:US-ASCII>
UTF-16BE	UTF-16BE,UCS-2BE	false	false	#<Encoding:UTF-16BE>
UTF-16LE	UTF-16LE	false	false	#<Encoding:UTF-16LE>
UTF-32BE	UTF-32BE,UCS-4BE	false	false	#<Encoding:UTF-32BE>
UTF-32LE	UTF-32LE,UCS-4LE	false	false	#<Encoding:UTF-32LE>
UTF-16	UTF-16	true	false	#<Encoding:UTF-16 (dummy)>
UTF-32	UTF-32	true	false	#<Encoding:UTF-32 (dummy)>
UTF8-MAC	UTF8-MAC,UTF-8-MAC,UTF-8-HFS	false	true	#<Encoding:UTF8-MAC>
EUC-JP	EUC-JP,eucJP	false	true	#<Encoding:EUC-JP>
Windows-31J	Windows-31J,CP932,csWindows31J,SJIS,PCK	false	true	#<Encoding:Windows-31J>
Big5	Big5	false	true	#<Encoding:Big5>
Big5-HKSCS	Big5-HKSCS,Big5-HKSCS:2008	false	true	#<Encoding:Big5-HKSCS>
Big5-UAO	Big5-UAO	false	true	#<Encoding:Big5-UAO>
CESU-8	CESU-8	false	true	#<Encoding:CESU-8>
CP949	CP949	false	true	#<Encoding:CP949>
Emacs-Mule	Emacs-Mule	false	true	#<Encoding:Emacs-Mule>
EUC-KR	EUC-KR,eucKR	false	true	#<Encoding:EUC-KR>
EUC-TW	EUC-TW,eucTW	false	true	#<Encoding:EUC-TW>
GB18030	GB18030	false	true	#<Encoding:GB18030>
GBK	GBK,CP936	false	true	#<Encoding:GBK>
ISO-8859-1	ISO-8859-1,ISO8859-1	false	true	#<Encoding:ISO-8859-1>
ISO-8859-2	ISO-8859-2,ISO8859-2	false	true	#<Encoding:ISO-8859-2>
ISO-8859-3	ISO-8859-3,ISO8859-3	false	true	#<Encoding:ISO-8859-3>
ISO-8859-4	ISO-8859-4,ISO8859-4	false	true	#<Encoding:ISO-8859-4>
ISO-8859-5	ISO-8859-5,ISO8859-5	false	true	#<Encoding:ISO-8859-5>
ISO-8859-6	ISO-8859-6,ISO8859-6	false	true	#<Encoding:ISO-8859-6>
ISO-8859-7	ISO-8859-7,ISO8859-7	false	true	#<Encoding:ISO-8859-7>
ISO-8859-8	ISO-8859-8,ISO8859-8	false	true	#<Encoding:ISO-8859-8>
ISO-8859-9	ISO-8859-9,ISO8859-9	false	true	#<Encoding:ISO-8859-9>
ISO-8859-10	ISO-8859-10,ISO8859-10	false	true	#<Encoding:ISO-8859-10>
ISO-8859-11	ISO-8859-11,ISO8859-11	false	true	#<Encoding:ISO-8859-11>
ISO-8859-13	ISO-8859-13,ISO8859-13	false	true	#<Encoding:ISO-8859-13>
ISO-8859-14	ISO-8859-14,ISO8859-14	false	true	#<Encoding:ISO-8859-14>
ISO-8859-15	ISO-8859-15,ISO8859-15	false	true	#<Encoding:ISO-8859-15>
ISO-8859-16	ISO-8859-16,ISO8859-16	false	true	#<Encoding:ISO-8859-16>
KOI8-R	KOI8-R,CP878	false	true	#<Encoding:KOI8-R>
KOI8-U	KOI8-U	false	true	#<Encoding:KOI8-U>
Shift_JIS	Shift_JIS	false	true	#<Encoding:Shift_JIS>
Windows-1250	Windows-1250,CP1250	false	true	#<Encoding:Windows-1250>
Windows-1251	Windows-1251,CP1251	false	true	#<Encoding:Windows-1251>
Windows-1252	Windows-1252,CP1252	false	true	#<Encoding:Windows-1252>
Windows-1253	Windows-1253,CP1253	false	true	#<Encoding:Windows-1253>
Windows-1254	Windows-1254,CP1254	false	true	#<Encoding:Windows-1254>
Windows-1257	Windows-1257,CP1257	false	true	#<Encoding:Windows-1257>
IBM437	IBM437,CP437	false	true	#<Encoding:IBM437>
IBM720	IBM720,CP720	false	true	#<Encoding:IBM720>
IBM737	IBM737,CP737	false	true	#<Encoding:IBM737>
IBM775	IBM775,CP775	false	true	#<Encoding:IBM775>
CP850	CP850,IBM850	false	true	#<Encoding:CP850>
IBM852	IBM852	false	true	#<Encoding:IBM852>
CP852	CP852	false	true	#<Encoding:CP852>
IBM855	IBM855	false	true	#<Encoding:IBM855>
CP855	CP855	false	true	#<Encoding:CP855>
IBM857	IBM857,CP857	false	true	#<Encoding:IBM857>
IBM860	IBM860,CP860	false	true	#<Encoding:IBM860>
IBM861	IBM861,CP861	false	true	#<Encoding:IBM861>
IBM862	IBM862,CP862	false	true	#<Encoding:IBM862>
IBM863	IBM863,CP863	false	true	#<Encoding:IBM863>
IBM864	IBM864,CP864	false	true	#<Encoding:IBM864>
IBM865	IBM865,CP865	false	true	#<Encoding:IBM865>
IBM866	IBM866,CP866	false	true	#<Encoding:IBM866>
IBM869	IBM869,CP869	false	true	#<Encoding:IBM869>
Windows-1258	Windows-1258,CP1258	false	true	#<Encoding:Windows-1258>
GB1988	GB1988	false	true	#<Encoding:GB1988>
macCentEuro	macCentEuro	false	true	#<Encoding:macCentEuro>
macCroatian	macCroatian	false	true	#<Encoding:macCroatian>
macCyrillic	macCyrillic	false	true	#<Encoding:macCyrillic>
macGreek	macGreek	false	true	#<Encoding:macGreek>
macIceland	macIceland	false	true	#<Encoding:macIceland>
macRoman	macRoman	false	true	#<Encoding:macRoman>
macRomania	macRomania	false	true	#<Encoding:macRomania>
macThai	macThai	false	true	#<Encoding:macThai>
macTurkish	macTurkish	false	true	#<Encoding:macTurkish>
macUkraine	macUkraine	false	true	#<Encoding:macUkraine>
CP950	CP950	false	true	#<Encoding:CP950>
CP951	CP951	false	true	#<Encoding:CP951>
IBM037	IBM037,ebcdic-cp-us	true	false	#<Encoding:IBM037 (dummy)>
stateless-ISO-2022-JP	stateless-ISO-2022-JP	false	true	#<Encoding:stateless-ISO-2022-JP>
eucJP-ms	eucJP-ms,euc-jp-ms	false	true	#<Encoding:eucJP-ms>
CP51932	CP51932	false	true	#<Encoding:CP51932>
EUC-JIS-2004	EUC-JIS-2004,EUC-JISX0213	false	true	#<Encoding:EUC-JIS-2004>
GB2312	GB2312,EUC-CN,eucCN	false	true	#<Encoding:GB2312>
GB12345	GB12345	false	true	#<Encoding:GB12345>
ISO-2022-JP	ISO-2022-JP,ISO2022-JP	true	false	#<Encoding:ISO-2022-JP (dummy)>
ISO-2022-JP-2	ISO-2022-JP-2,ISO2022-JP2	true	false	#<Encoding:ISO-2022-JP-2 (dummy)>
CP50220	CP50220	true	false	#<Encoding:CP50220 (dummy)>
CP50221	CP50221	true	false	#<Encoding:CP50221 (dummy)>
Windows-1256	Windows-1256,CP1256	false	true	#<Encoding:Windows-1256>
Windows-1255	Windows-1255,CP1255	false	true	#<Encoding:Windows-1255>
TIS-620	TIS-620	false	true	#<Encoding:TIS-620>
Windows-874	Windows-874,CP874	false	true	#<Encoding:Windows-874>
MacJapanese	MacJapanese,MacJapan	false	true	#<Encoding:MacJapanese>
UTF-7	UTF-7,CP65000	true	false	#<Encoding:UTF-7 (dummy)>
UTF8-DoCoMo	UTF8-DoCoMo	false	true	#<Encoding:UTF8-DoCoMo>
SJIS-DoCoMo	SJIS-DoCoMo	false	true	#<Encoding:SJIS-DoCoMo>
UTF8-KDDI	UTF8-KDDI	false	true	#<Encoding:UTF8-KDDI>
SJIS-KDDI	SJIS-KDDI	false	true	#<Encoding:SJIS-KDDI>
ISO-2022-JP-KDDI	ISO-2022-JP-KDDI	true	false	#<Encoding:ISO-2022-JP-KDDI (dummy)>
stateless-ISO-2022-JP-KDDI	stateless-ISO-2022-JP-KDDI	false	true	#<Encoding:stateless-ISO-2022-JP-KDDI>
UTF8-SoftBank	UTF8-SoftBank	false	true	#<Encoding:UTF8-SoftBank>
SJIS-SoftBank	SJIS-SoftBank	false	true	#<Encoding:SJIS-SoftBank>
name_list: 175
aliases: 71
constants: 211
Windows_1250: #<Encoding:Windows-1250>
WINDOWS_1250: #<Encoding:Windows-1250>
EucJP: #<Encoding:EUC-JP>
EUCJP: #<Encoding:EUC-JP>
EBCDIC_CP_US: #<Encoding:IBM037 (dummy)>
Big5_HKSCS_2008: #<Encoding:Big5-HKSCS>
no 646 constant: false
UNICODE_VERSION: "17.0.0"
default_external names: ["UTF-8", "CP65001", "locale", "external", "filesystem"]
internal in name_list: true
aliases has internal: false
find external: #<Encoding:UTF-8>
find internal: nil
find Latin-1: ArgumentError: unknown encoding name - Latin-1
ISO-8859-5 cyrillic: "р"
KOI8-U ghe: "ґ"
IBM437 box: "█"
CP866 be: "р"
macRoman omega: "Ω"
TIS-620 thai: "ก"
Windows-874 thai: "ก"
Windows-1258 dong: Encoding::ConverterNotFoundError: code converter not found (Windows-1258 to UTF-8)
into ISO-8859-7: [225, 226]
into IBM866: [128, 129]
ISO-8859-3 unassigned: Encoding::UndefinedConversionError: "\xA5" from ISO-8859-3 to UTF-8
ISO-8859-3 valid: true
ISO-8859-3 length: 2
ISO-8859-9 upcase: [73]
EUC-KR ascii: [97, 98, 99]
EUC-KR ascii back: "abc"
EUC-KR compatible?: #<Encoding:UTF-8>
EUC-KR names: ["EUC-KR", "eucKR"]
EUC-KR dummy?: false
EUC-KR ascii_compatible?: true
