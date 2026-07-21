# Issue #889: Array#pack and String#unpack with the common
# numeric/string format specifiers.
puts "ABC".unpack("C*").inspect
puts [65, 66, 67].pack("C*")

# Multi-spec format.
puts [255, 0x1234].pack("Cn").bytes.inspect
puts "\xff\x12\x34".unpack("Cn").inspect

# String specs.
puts ["hello", 42].pack("a5N").bytes.length    # 5 + 4 = 9
puts "hello\x00\x00\x00*".unpack("a5N").inspect
