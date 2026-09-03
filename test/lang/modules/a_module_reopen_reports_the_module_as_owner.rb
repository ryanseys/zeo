# Reopening a BUILTIN MODULE and asking who owns the method.
#
# A `module Comparable; def ctag; end; end` reopen flattens its row onto every
# class in the chain, so dispatch is one probe rather than a walk. Each of
# those classes registers a row of its own -- and every one of them claimed
# the name as its OWN, so `#owner` named `Integer` where ruby names
# `Comparable`, and `Integer.instance_methods(false)` listed it.
#
# A flattened row is already marked FOREIGN, because `super` must not find one
# at the position it is resuming from. `#owner` and `instance_methods(false)`
# ask that same question, and now read that same mark.

module Mine
  def mtag = :m
end
module Comparable
  def ctag = :c
end
module Enumerable
  def etag = :e
end
class Widget
  include Mine
end

def show(label)
  puts format("%-32s %s", label, (yield).inspect)
rescue => e
  puts format("%-32s %s", label, e.class)
end

show("user module owner")       { Widget.new.method(:mtag).owner }
show("user module unbound")     { Widget.instance_method(:mtag).owner }
show("user module own?")        { Widget.instance_methods(false).include?(:mtag) }
show("declared on the module")  { Mine.instance_methods(false).include?(:mtag) }

show("Comparable owner")        { 5.method(:ctag).owner }
show("through String")          { "s".method(:ctag).owner }
show("through Float")           { 1.5.method(:ctag).owner }
show("Comparable unbound")      { Integer.instance_method(:ctag).owner }
show("Integer own?")            { Integer.instance_methods(false).include?(:ctag) }
show("Comparable own?")         { Comparable.instance_methods(false).include?(:ctag) }
show("Comparable defines?")     { Comparable.method_defined?(:ctag) }

show("Enumerable owner")        { [].method(:etag).owner }
show("through Hash")            { {}.method(:etag).owner }
show("through Range")           { (1..2).method(:etag).owner }
show("Array own?")              { Array.instance_methods(false).include?(:etag) }

# The reopened row still RUNS, from every includer.
show("call through Integer")    { 5.ctag }
show("call through Array")      { [].etag }
show("call through Widget")     { Widget.new.mtag }
show("no super above it")       { 5.method(:ctag).super_method.inspect }
__END__
user module owner                Mine
user module unbound              Mine
user module own?                 false
declared on the module           true
Comparable owner                 Comparable
through String                   Comparable
through Float                    Comparable
Comparable unbound               Comparable
Integer own?                     false
Comparable own?                  true
Comparable defines?              true
Enumerable owner                 Enumerable
through Hash                     Enumerable
through Range                    Enumerable
Array own?                       false
call through Integer             :c
call through Array               :e
call through Widget              :m
no super above it                "nil"
