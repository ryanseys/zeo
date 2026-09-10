# The uninitialized-constant message is namespaced ("Object::AlsoMissing");
# ruby names the constant alone.
#
# A constant the whole program never defines is warned about at build time and
# still raises NameError when reached, which is what CRuby does and what
# ruby/spec asserts (#3976). The warning is what a dropped `require` needs: the
# build used to say nothing at all.
p defined?(Nope)
p defined?(Nope::Deep)

begin
  p Missing
rescue NameError => e
  p e.message
end

begin
  p Object::AlsoMissing
rescue NameError => e
  p e.message
end

X = 5
p X
p Integer
class Y; end
p Y
__END__
nil
nil
"uninitialized constant Missing"
"uninitialized constant AlsoMissing"
5
Integer
Y
