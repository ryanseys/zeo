# JSON.generate refuses what is not valid UTF-8: an invalid-UTF-8 string
# raises JSON::GeneratorError ("source sequence is illegal/malformed
# utf-8"), and a binary string raises on the transcode. zeo lossy-
# replaces the first and transcodes the second -- silent data
# corruption. The float form also differs: 1e100.to_json is "1e+100",
# not "1.0e+100". (Found by the 2026-08-24 probe sweep.)
require "json"
def show
  p yield
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end
show { JSON.generate(["\xFF".b.force_encoding("UTF-8")]) }
show { JSON.generate(["\xE9".b]) }
show { 1e100.to_json }
