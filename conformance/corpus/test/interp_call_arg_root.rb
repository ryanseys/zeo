# Regression for #1498: a module_function whose String return is inferred
# nilable, called with a string-literal arg from inside a string interpolation
# that is passed to Regexp.new/compile. The embedded call roots its own arg, and
# that rooting decl must land as a whole statement before the pattern temp's
# declaration -- not nested inside its initializer (which emitted illegal C:
# `const char *_t3 = const char *_t5 = ...`).
module StringGlob
  module_function
  def regexp_string(s)
    return nil if s.empty?
    s.gsub("*", ".*")
  end
end

re1 = Regexp.new("\\A#{StringGlob.regexp_string('*.rb')}\\z")
puts re1 === 'hello.rb'
puts re1 === 'hello.txt'

re2 = Regexp.compile("\\A#{StringGlob.regexp_string('*.txt')}\\z")
puts re2 === 'notes.txt'
