# `def v; end; def v; end` fires `method_added(:v)` twice, and at the FIRST
# firing ruby has only the first body installed. zeo runs the second.
#
# This is not the watermark, which handles a name whose first definition is
# still ahead (`tests/method_added_dispatch_watermark.rb`). Here `v` is already
# defined at both firings -- what differs is WHICH body. zeo keeps one row per
# name: `analyze::add_own_method` REPLACES an earlier same-name entry rather
# than appending, which is Ruby's own last-def-wins rule everywhere else, and
# codegen emits one method per surviving row. The superseded body is not
# compiled at all, so there is nothing for the first hook to reach.
#
# Fix shape, if this ever earns it: keep every redefinition as its own row,
# emit them all, and have the watermark name which row is current at each hook
# position. That is a per-redefinition cost in generated code paid by every
# program, to serve a hook body that reads a method it is about to lose.

class Redef
  def self.method_added(name)
    return unless name == :v
    puts "  v -> #{new.v}"
  end

  def v = 1
  def v = 2
end
p Redef.new.v

# The same asymmetry with an `attr_accessor` superseded by a hand-written
# reader: the hook for `:name` sees the generated one in ruby.
class Attr
  def self.method_added(m)
    return unless m == :name
    puts "  name -> #{new.name.inspect}"
  end

  attr_accessor :name
  def name = "hand-written"
end
p Attr.new.name
