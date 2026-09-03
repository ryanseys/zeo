# An inner `begin` (loop labels cleared) `?`-propagates the `break` up to
# the outer `begin`, which -- inline in the loop -- performs the literal
# translation. The crossing scan descends THROUGH nested begins (H2).

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
puts "out"
__END__
inner 0
inner 1
out
