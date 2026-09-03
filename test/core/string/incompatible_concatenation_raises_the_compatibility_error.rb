# Two differently-encoded NON-7-bit strings; CRuby's exact message,
# receiver's encoding first in its `inspect` form. `<<` past 255 on a
# byte-encoded receiver is the RangeError, not a promotion.

begin
  "é".b + "é"
rescue Encoding::CompatibilityError => e
  puts "plus: #{e.message}"
end
begin
  ("é".b) << "é"
rescue Encoding::CompatibilityError => e
  puts "shovel: #{e.message}"
end
begin
  "".b << 0x1F600
rescue RangeError => e
  puts "range: #{e.message}"
end
__END__
plus: incompatible character encodings: BINARY (ASCII-8BIT) and UTF-8
shovel: incompatible character encodings: BINARY (ASCII-8BIT) and UTF-8
range: 128512 out of char range
