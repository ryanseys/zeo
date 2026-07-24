h = {"int" => -9}
h["array"] = [1, 2]
h["bool"] = false
h["float"] = 1.5
h["hash"] = {"nested" => 7}
h["nil"] = nil
h["string"] = "text"

h.keys.sort.each do |key|
  value = h[key]
  rendered = if value.is_a?(Array)
    "array:" + value.length.to_s
  elsif value.is_a?(Float)
    "float:" + value.to_s
  elsif value.is_a?(Integer)
    "int:" + value.abs.to_s
  elsif value.is_a?(String)
    "string:" + value.upcase
  elsif value.is_a?(Hash)
    "hash"
  elsif value.nil?
    "nil"
  else
    "bool:" + value.to_s
  end
  puts key + "=" + rendered
end
