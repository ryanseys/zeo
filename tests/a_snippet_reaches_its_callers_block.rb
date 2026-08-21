# `yield`, `block_given?` and a bare `super` written at a snippet's own
# level belong to the method the `eval` was called from -- CRuby reads them
# off the caller's control frame. zeo publishes the home from every scope
# that lexically contains a run-time `eval`, so the snippet finds it
# whichever eval surface ran it.

YIELD = "yield 7"
GIVEN = "block_given?"
SUPER = "super"
DEF_S = "defined?(super)"
DEF_Y = "defined?(yield)"

def plain = eval(YIELD)
def given = eval(GIVEN)
def through_instance_eval = instance_eval(YIELD)
def through_class_eval = self.class.class_eval(YIELD)
def through_binding = eval(YIELD, binding)

p(plain { |x| x * 3 })
p given
p(given { })
p(through_instance_eval { |x| "ie #{x}" })
p(through_class_eval { |x| "ce #{x}" })
p(through_binding { |x| "b #{x}" })

# A block re-publishes the home it inherited, so an intervening method that
# can eval cannot shadow it.
def helper
  eval(["1", "2"].first)
  yield
end
def outer
  helper { p eval(YIELD) }
end
outer { |x| "outer got #{x}" }

def nested
  [1].map { [2].map { eval(YIELD) } }
end
p(nested { |x| x + 100 })

# A bare `super` forwards the enclosing method's own arguments.
class Base
  def go(a, b = 2, *rest, k: 9) = [a, b, rest, k]
end
class Sub < Base
  def go(a, b = 20, *rest, k: 90) = eval(SUPER)
end
p Sub.new.go(1)
p Sub.new.go(1, 2, 3, k: 4)

# `defined?` answers the same questions.
class DBase; def d = 1; end
class DSub < DBase; def d = eval(DEF_S); end
class DSolo; def d = eval(DEF_S); end
p DSub.new.d
p DSolo.new.d
p eval(DEF_S)
def dy = eval(DEF_Y)
p dy
p(dy { })

# A method that HAS no block raises `LocalJumpError` when the snippet
# yields; an `eval` called from a scope that can never have one -- the top
# level, a class body -- does not compile at all, which is CRuby's own
# `Invalid yield` and carries the eval's location as its whole backtrace.
def blockless = eval(YIELD)
begin
  blockless
rescue LocalJumpError => e
  puts "#{e.class}: #{e.message}"
end
begin
  eval(YIELD)
rescue SyntaxError => e
  puts "#{e.class}: #{e.message}"
  p e.backtrace
end
class InBody
  begin
    eval(YIELD)
  rescue SyntaxError => e
    puts "class body: #{e.message}"
  end
end
# `defined?(yield)` asks rather than yields, so it answers nil instead.
p eval(DEF_Y)
