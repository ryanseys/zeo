# A bare `break`/`next` inside a `begin`/`rescue` targets the native loop
# written OUTSIDE the `begin` (Batch H). The `begin` expression is spliced
# inline in the loop body, so its final settling translates the bubbled
# control-flow signal into the loop's own literal jump -- `ensure` still runs
# exactly once on the way out, and a re-raised exception still propagates.

# break out of a while loop from the begin body
i = 0
while i < 5
  begin
    break if i == 3
    puts "body #{i}"
  rescue
  end
  i += 1
end
puts "after while"

# next from a rescue clause continues the loop
i = 0
while i < 5
  i += 1
  begin
    raise "even" if i.even?
    puts "odd #{i}"
  rescue
    next
  end
end

# ensure runs before the break crosses out
i = 0
while i < 4
  begin
    break if i == 2
    puts "b#{i}"
  ensure
    puts "e#{i}"
  end
  i += 1
end

# a break inside a nested begin bubbles through to the outer loop
i = 0
while i < 4
  begin
    begin
      break if i == 2
      puts "inner #{i}"
    rescue
    end
  rescue
  end
  i += 1
end
puts "after nested"

# a re-raised exception is still free to propagate past the loop
begin
  i = 0
  while i < 3
    begin
      raise "boom" if i == 1
    rescue => e
      raise "rethrow #{e.message}"
    end
    i += 1
  end
rescue => e
  puts "caught: #{e.message}"
end
__END__
body 0
body 1
body 2
after while
odd 1
odd 3
odd 5
b0
e0
b1
e1
e2
inner 0
inner 1
after nested
caught: rethrow boom
