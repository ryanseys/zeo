# `p` on a user exception subclass renders like #inspect does, not as the
# object default (#3813).
class E1 < StandardError; end
class E2 < StandardError
  def initialize(m = "d"); super; end
end
p(E1.new("m"))
p(E1.new("m").inspect)
p(E1.new)
p(E2.new)
p(RuntimeError.new("via p"))
a = [E1.new("in array")]
p a
