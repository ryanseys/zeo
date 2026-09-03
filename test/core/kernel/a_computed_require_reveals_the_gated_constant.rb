# Loading a feature is only half of a `require`: the constant it gates has to
# start existing at that line, and not before.
#
# zeo registers a require-gated builtin's class only when some file requires
# its feature, so a program whose only require is COMPUTED registered nothing
# and the constant stayed a `NameError` even after the load succeeded. The
# compiler now registers every gated builtin for such a program, CONCEALED --
# which is the shape an activated one already had -- so the constant is absent
# until the require reveals it, exactly as in ruby.

gated = %w[StringIO StringScanner BigDecimal Etc]

def looked_up(name)
  Object.const_get(name)
  "found"
rescue NameError
  "absent"
end

puts "before"
gated.each { |n| puts "  #{n}\t#{looked_up(n)}" }

%w[stringio strscan bigdecimal etc].each { |f| require f }

puts "after"
gated.each { |n| puts "  #{n}\t#{Object.const_get(n)}" }

# And the classes work, not merely resolve.
puts StringIO.new("hello").read
puts StringScanner.new("ab12").scan(/[a-z]+/)
puts BigDecimal("1.25") + BigDecimal("0.25")
puts Etc.respond_to?(:getpwuid)
__END__
before
  StringIO	absent
  StringScanner	absent
  BigDecimal	absent
  Etc	absent
after
  StringIO	StringIO
  StringScanner	StringScanner
  BigDecimal	BigDecimal
  Etc	Etc
hello
ab
0.15e1
true
