# The bubbled `break` is translated only AFTER `ensure` runs, exactly once
# -- the loop-crossing settle sits below the ensure block (H2).

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
__END__
b0
e0
b1
e1
e2
