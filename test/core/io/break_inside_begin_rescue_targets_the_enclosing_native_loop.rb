# A bare `break`/`next`/`redo` inside a `begin`/`rescue`/`else` clause
# targeting a native loop OUTSIDE the `begin`. The `begin`
# expression is spliced INLINE in the loop body, so its final settling
# translates the bubbled `Signal` into the loop's own literal jump -- no
# loop-body closure needed (see `clif::control::lower_begin`). A
# loop written INSIDE the `begin` is unaffected (the previous test).

i = 0
while i < 5
  begin
    break if i == 3
    puts "body #{i}"
  rescue
  end
  i += 1
end
puts "after"
__END__
body 0
body 1
body 2
after
