#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
b = Ruby::Box.new
b.eval("class Array; def self.zzz = 9; end")
p Array.respond_to?(:zzz)
__END__
false
