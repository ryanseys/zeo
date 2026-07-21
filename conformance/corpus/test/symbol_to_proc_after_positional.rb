# &:symbol block-pass after a positional argument must parse as a
# symbol-to-proc block, not a hash literal -- in both parenthesized
# and command (paren-less) calls. Front-door blocker for bundler
# (File.open(f, "r:UTF-8", &:read)) and minitest
# (define_method :mu_pp, &:pretty_inspect).
class Widget
  define_method(:as_str, &:to_s)   # parenthesized, after a positional
end

class Gadget
  define_method :label, &:to_s     # command (paren-less), after a positional
end

puts Widget.new.as_str(7)
puts Gadget.new.label(8)
p [1, 2, 3].map(&:to_s)            # sole arg in parens (regression guard)
p [1, 2, 3].map &:to_s            # command-position sole arg
p [10, 20, 30].inject(0, &:+)     # operator symbol after a positional (native)
