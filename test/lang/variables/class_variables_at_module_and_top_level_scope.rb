# `@@x` written in a module body and read back through its own `def self.`
# accessors. Bare at the TOP LEVEL it raises instead -- see
# `top_level_class_variable_raises` below.

module Conf
  @@secret = ""
  def self.secret; @@secret; end
  def self.secret=(v); @@secret = v; end
end
puts Conf.secret.length
Conf.secret = "hi"
puts Conf.secret
__END__
0
hi
