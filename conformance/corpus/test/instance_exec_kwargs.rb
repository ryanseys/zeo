# Keyword block params bind by name from the call-site `key:` args. An
# optional param falls back to its default expression when the call
# omits it; a positional param can precede the keyword params.

class Calc
  def initialize(base)
    @base = base
  end

  def base
    @base
  end
end

c = Calc.new(100)

# Required keyword params, both supplied.
puts(c.instance_exec(k: 9, j: 2) { |k:, j:| base + k + j })   #=> 111

# Optional param supplied overrides its default.
puts(c.instance_exec(k: 1, j: 7) { |k:, j: 5| k + j })        #=> 8

# Optional param omitted uses its default.
puts(c.instance_exec(k: 1) { |k:, j: 5| k + j })              #=> 6

# Mixed: a positional param plus a keyword param.
puts(c.instance_exec(10, k: 3) { |a, k:| base + a + k })      #=> 113

puts "done"
