# `next` from a `rescue` clause -> `continue` the enclosing loop (H2).

i = 0
while i < 5
  i += 1
  begin
    raise "x" if i.even?
    puts "odd #{i}"
  rescue
    next
  end
end
__END__
odd 1
odd 3
odd 5
