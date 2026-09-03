# The `begin`'s own `retry` restarts just its OWN body (not the `while`
# loop), and `ensure` runs once per `while` ITERATION (twice total,
# once per `i`) -- not once per `retry` attempt within an iteration.

total_ensure = 0
i = 0
while i < 2
  attempts = 0
  begin
    attempts += 1
    raise "x" if attempts < 2
  rescue
    retry if attempts < 2
  ensure
    total_ensure += 1
  end
  i += 1
end
puts total_ensure
__END__
2
