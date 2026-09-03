# CRuby raises wrong-number-of-arguments / missing-keyword INSIDE the
# callee: the innermost backtrace row is the callee's label at its
# `def` line, and an `initialize`-less `.new` shows the C-frame shape
# `'BasicObject#initialize'` at the CALLER's line. This battery was
# verified verbatim against ruby 4.0.6 (the arity family was previously
# an excluded, catalogued divergence of the frame-tracking battery).

def f(a, b)
  a + b
end
begin
  f(1)
rescue ArgumentError => e
  puts e.message
  puts e.backtrace.first.split('/').last
end
class Bag
  def initialize(x)
    @x = x
  end
end
begin
  Bag.new(1, 2, 3)
rescue ArgumentError => e
  puts e.message
  puts e.backtrace.first.split('/').last
end
class Plain; end
begin
  Plain.new(5)
rescue ArgumentError => e
  puts e.message
  puts e.backtrace.first.split('/').last
end
def kw(x:)
  x
end
begin
  kw
rescue ArgumentError => e
  puts e.message
  puts e.backtrace.first.split('/').last
end
__END__
wrong number of arguments (given 1, expected 2)
arity_errors_attribute_to_the_callee_frame_like_cruby.rb:8:in 'Object#f'
wrong number of arguments (given 3, expected 1)
arity_errors_attribute_to_the_callee_frame_like_cruby.rb:18:in 'Bag#initialize'
wrong number of arguments (given 1, expected 0)
arity_errors_attribute_to_the_callee_frame_like_cruby.rb:30:in 'BasicObject#initialize'
missing keyword: :x
arity_errors_attribute_to_the_callee_frame_like_cruby.rb:35:in 'Object#kw'
