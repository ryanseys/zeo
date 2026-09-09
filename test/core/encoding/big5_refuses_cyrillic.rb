# Encoding Cyrillic to Big5: CRuby's strict Big5 has no Cyrillic rows and
# refuses with UndefinedConversionError; zeo's WHATWG table maps them (the
# HKSCS-adjacent region), so it encodes MORE than ruby. The inverse
# direction of the decided Big5-HKSCS divergence in docs/COMPATIBILITY.md.
begin
  p "Жи".encode("Big5").unpack1("H*")
rescue Encoding::UndefinedConversionError => e
  puts "#{e.class}: #{e.message}"
end
__END__
Encoding::UndefinedConversionError: U+0416 from UTF-8 to Big5
