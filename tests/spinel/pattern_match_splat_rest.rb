# Issue #844: array pattern with `*rest` splat target. The rest LV
# needs to be declared in the surrounding scope; pre-fix it was
# only handled at codegen-emit, leaving lv_rest undeclared.
case [1, 2, 3]
in [first, *rest]
  puts "first=" + first.to_s
  puts "rest=" + rest.inspect
end

# `Array(...)` with capture constant + rest
case [10, 20, 30]
in Array(head, *tail)
  puts "head=" + head.to_s
  puts "tail=" + tail.inspect
end
