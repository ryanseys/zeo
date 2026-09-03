# A custom `yield`-based method attaches a real escaping block here
# directly, exercising the same closure boundary `Array#each` would.
# The begin/rescue
# lowering (see `clif::control::lower_begin`) needs no
# special handling: the block passed to `each_num` is ALREADY a real
# escaping `Proc` (its own closure boundary, `in_real_proc` already
# true), so a `next` inside the nested `begin`'s rescue clause raises
# the exact same `Signal` it already would have -- caught by the Proc's
# own wrapper loop, not by the (irrelevant here) native-loop rejection
# check.

class Each3
  def each_num
    yield 1
    yield 2
    yield 3
  end
end

Each3.new.each_num do |i|
  begin
    raise "boom" if i == 2
    puts "ok #{i}"
  rescue
    next
  ensure
    puts "ensure #{i}"
  end
end
__END__
ok 1
ensure 1
ensure 2
ok 3
ensure 3
