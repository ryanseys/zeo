# `raise <non-exception>` coerces at runtime -- a String becomes a
# RuntimeError, everything else is CRuby's TypeError -- instead of
# panicking when the raise machinery unwraps a non-Object.

def try
  yield
rescue TypeError => e
  puts "TypeError: #{e.message}"
end
try { raise 42 }
try { raise nil }
try { raise :sym }
begin
  raise "boom"
rescue RuntimeError => e
  puts "RuntimeError: #{e.message}"
end
mixed = [Object.new, "msg"]
begin
  raise mixed[1]
rescue RuntimeError => e
  puts "poly string: #{e.message}"
end
begin
  raise mixed[0]
rescue TypeError => e
  puts "poly object: #{e.message}"
end
__END__
TypeError: exception class/object expected
TypeError: exception class/object expected
TypeError: exception class/object expected
RuntimeError: boom
poly string: msg
poly object: exception class/object expected
