# `class << obj; MAX = ...; def m; MAX; end; end` -- a constant on an
# object's singleton class (tmpdir's `class << RANDOM`). zeo hoists the
# constant to the enclosing lexical scope, where the singleton method
# resolves it.

module Holder
  GEN = Object.new
  class << GEN
    STEP = 6
    def emit = STEP * 7
  end
end
puts Holder::GEN.emit
__END__
42
