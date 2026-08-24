# The five `NilClass` rows CRuby writes in RUBY, answered by CRuby's own bytes.
#
# CRuby compiles 24 of its core files into the interpreter (`BUILTIN_RB_SRCS`),
# and a method defined in one reports an `<internal:>` name rather than a path.
# zeo vendors `nilclass.rb` verbatim from the same `ruby/ruby` pin the C API
# headers ride and compiles it in ahead of the program's first statement, so
# every answer below is upstream's own code rather than a Rust twin kept in
# step by hand.
#
# What that closes here: five `source_location` rows that answered `nil`, and
# `Method#inspect`, which rendered `rationalize(*)` where ruby writes
# `rationalize(eps=...)` -- the arity-derived descriptor a native row falls
# back to cannot name a parameter it never had.
#
# `ZEO_CORELIB=rust` answers from `builtins/nil_class.rs` instead. The corpus
# runs in both modes; see docs/CORELIB.md.
p nil.to_i
p nil.to_f
p nil.to_r
p nil.to_c
p nil.rationalize
p nil.rationalize(0.5)
p nil.rationalize(nil)

# The `<internal:nilclass>` attribution, and the line each `def` stands on in
# upstream's file. A wrong line here means the segment was lowered without its
# own source registered.
%i[to_i to_f to_r to_c rationalize].each do |m|
  u = NilClass.instance_method(m)
  p [m, u.arity, u.parameters, u.source_location]
end
p nil.method(:to_i).inspect
p nil.method(:rationalize).inspect

# A C-defined row still answers nil, exactly as ruby's does -- the corelib is
# five rows, not the whole class.
p NilClass.instance_method(:to_s).source_location
p NilClass.instance_methods(false).sort

# The rows behave, not just reflect.
p nil.to_r.class
p nil.to_c.real
p nil.to_i + 1
p [nil.to_f, nil.to_f.class]
