# The receiver isn't a literal constant, so codegen can't emit a direct
# `H1::__cm_run(...)` -- it goes through the registry's class-method
# table on a `RubyValue::Class` receiver.

class H1
  def self.run(x); "h1:#{x}"; end
end
class H2
  def self.run(x); "h2:#{x}"; end
end
[H1, H2].each { |h| puts h.run(5) }
handler = H1
puts handler.run(9)
handler = H2
puts handler.run(9)
__END__
h1:5
h2:5
h1:9
h2:9
