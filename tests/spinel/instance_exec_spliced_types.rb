# String, symbol, and mixed-type block params inside a spliced
# trampoline body.
#
# The trampoline-pattern inliner splices the block body at the
# call site with each block param bound to a per-call-site C
# local (e.g. `s_ix1` instead of `s`). For `puts <s>` and similar
# type-aware dispatch inside the spliced body to format the
# value correctly, the analyzer must resolve `s`'s type to
# "string" via find_var_type. Previously only the suffixed name
# (`s_ix1`) was declared in the splice scope, so find_var_type
# of the original `s` returned "" and puts defaulted to integer
# format -- string params printed as garbage ints.
#
# Now the splice declares each block param under both names
# (`s_ix1` AND `s`), and the splice scope is bracketed by
# push_scope / pop_scope so the original-name declaration shadows
# outer locals only inside the splice. Outer locals with the
# same name (e.g. an outer `b = Builder.new` and a block param
# `|b|`) are restored after the splice via pop_scope.
#
# Tag has a subclass (TagPlus) so Spinel keeps it heap-allocated
# (multi-instance classes can't be SRA-promoted to value types).

class Tag
  def fire(s, y, n, &b)
    instance_exec(s, y, n, &b)
  end
end

class TagPlus < Tag
end

t = Tag.new
t.fire("hi", :sym, 7) { |s, y, n| puts s; puts y; puts n }

# Block-param name shadowing an outer local: `b` (the receiver)
# vs `|b|` (the second block param). After the splice, `b` must
# still be the Builder so subsequent dispatch works.
class Builder
  def initialize; @s = 0; end
  def add(n); @s = @s + n; end
  def total; @s; end
end

class BuilderPlus < Builder
end

b = Builder.new
t.fire("hi", :sym, 7) { |s, y, n| b.add(n) }   # `b` here is the outer Builder
t.fire("hi", :sym, 7) { |s, y, n| b.add(n) }

# Now actually shadow `b`: the middle block param is named `b`, so inside
# the splice `b` is the :sym param (prints "sym"), not the Builder. After
# the splice pop_scope restores `b`, so puts b.total below still sees the
# Builder. If the restore were missing, `b.total` would mis-resolve.
t.fire("hi", :sym, 7) { |s, b, n| puts b }
puts b.total                     # 14

puts "done"
