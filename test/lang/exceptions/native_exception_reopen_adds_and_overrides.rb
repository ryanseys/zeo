# Reopening a NATIVE exception class (D3): a reopen `def` ADDS a method that
# reaches every subclass, and OVERRIDES an existing native method everywhere.
# The native message lives in a hidden slot, so a reopened `message` reading
# `@message` sees nil (CRuby parity) -- here it reads `to_s`.

class StandardError
  def code
    42
  end
end
class Exception
  def message
    "patched: " + to_s
  end
end
begin
  raise ArgumentError, "bad value"
rescue => e
  puts e.code
  puts e.message
  puts e.is_a?(StandardError)
end
begin
  raise "plain"
rescue => e
  puts e.message
end
__END__
42
patched: bad value
true
patched: plain
