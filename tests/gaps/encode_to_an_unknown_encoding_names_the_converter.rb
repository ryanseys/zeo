# `String#encode` to an unknown encoding raises
# Encoding::ConverterNotFoundError naming the pair ("code converter not
# found (UTF-8 to NO-SUCH-ENC)"); zeo routes through the registry lookup
# first and raises ArgumentError ("unknown encoding name"). The Converter
# and Encoding.find paths already match -- only #encode misroutes. (Found
# by the 2026-08-24 probe sweep.)
begin
  "a".encode("NO-SUCH-ENC")
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end
begin
  Encoding::Converter.new("UTF-8", "NO-SUCH-ENC")
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end
begin
  Encoding.find("NO-SUCH-ENC")
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end
