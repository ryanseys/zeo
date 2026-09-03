# Integer#chr: CRuby-strict range check for the byte form (RangeError outside
# 0..255) and real UTF-8 encoding for chr(Encoding::UTF_8) -- previously the
# encoding argument was dropped and the codepoint silently truncated to one
# byte (0x3042.chr(UTF_8) yielded "B").
p 65.chr
p 0x3042.chr(Encoding::UTF_8)
p 0x80.chr(Encoding::UTF_8).bytes
p 0x7FF.chr(Encoding::UTF_8).bytes
p 0xFFFF.chr(Encoding::UTF_8).bytes
p 0x10FFFF.chr(Encoding::UTF_8).bytes
p 65.chr(Encoding::UTF_8)
p 65.chr(Encoding::US_ASCII)
p 200.chr(Encoding::ASCII_8BIT).bytes
[-1, 256, 0x3042].each do |n|
  begin
    n.chr
    puts "#{n}.chr: ok"
  rescue RangeError => e
    puts "#{n}.chr: RangeError: #{e.message}"
  end
end
[-1, 0xD800, 0x110000].each do |n|
  begin
    n.chr(Encoding::UTF_8)
    puts "#{n}.chr(UTF_8): ok"
  rescue RangeError => e
    puts "#{n}.chr(UTF_8): RangeError: #{e.message}"
  end
end
__END__
"A"
"あ"
[194, 128]
[223, 191]
[239, 191, 191]
[244, 143, 191, 191]
"A"
"A"
[200]
-1.chr: RangeError: -1 out of char range
256.chr: RangeError: 256 out of char range
12354.chr: RangeError: 12354 out of char range
-1.chr(UTF_8): RangeError: -1 out of char range
55296.chr(UTF_8): RangeError: invalid codepoint 0xD800 in UTF-8
1114112.chr(UTF_8): RangeError: 1114112 out of char range
