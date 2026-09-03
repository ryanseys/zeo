# A native loop CONTAINING a `begin`/`rescue` (as opposed to a `begin`
# containing a bare `break`/`next` TARGETING an outer loop, the
# rejected shape) is completely unaffected -- the `.times` loop's own
# control flow doesn't cross the begin/rescue closure boundary at all
# here, so it just runs normally, once per iteration.

3.times do |i|
  begin
    raise "x" if i == 1
    puts "ok #{i}"
  rescue
    puts "rescued #{i}"
  end
end
__END__
ok 0
rescued 1
ok 2
