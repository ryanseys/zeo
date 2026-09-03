outer_runs = 0
inner_attempts = 0
begin
  outer_runs += 1
  begin
    inner_attempts += 1
    raise "x" if inner_attempts < 2
    puts "inner ok after #{inner_attempts}"
  rescue
    retry if inner_attempts < 2
  end
  puts "outer ran #{outer_runs} times"
end
__END__
inner ok after 2
outer ran 1 times
