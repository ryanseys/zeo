#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
b = Ruby::Box.new

# The reopen's VALUE is used, so the splice sits in expression position.
x = b.eval("class String; def shout = upcase + '!'; end; 'a'.shout")
p x
p "a".respond_to?(:shout)

# ...and inside a block, which is the same nesting question.
[1].each { b.eval("class Array; def two = 2; end") }
p b.eval("[1].two")
p [1].respond_to?(:two)

# A class-level ivar written in a box body is the box's own.
b.eval("class Array; @zz = 7; def self.zz = @zz; end")
p b.eval("Array.zz")
p Array.instance_variable_get(:@zz)

# A box's `remove_method`/`undef_method` names its own rows, and retires
# them for that box alone.
p b.eval("class String; def gone = 1; end; String.instance_methods(false).include?(:gone)")
p b.eval("class String; remove_method :gone; end; 'a'.respond_to?(:gone)")
p b.eval("class String; undef_method :upcase; end; ('a'.upcase rescue $!.class.to_s)")
p "a".upcase

# A builtin MODULE mixed in inside a box reaches the OPERATOR form too.
c = Ruby::Box.new
c.eval("class Array; include Comparable; end")
p c.eval("[1] < [2]")
p c.eval("[1].send(:<, [2])")
p ([1] < [2] rescue $!.class.to_s)
__END__
"A!"
false
2
false
7
nil
true
false
"NoMethodError"
"A"
true
true
"NoMethodError"
