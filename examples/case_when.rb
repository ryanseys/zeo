day = :tue
case day
when :sat, :sun
  puts :weekend
when :mon, :tue, :wed, :thu, :fri
  puts :weekday
else
  puts :unknown
end

n = 7
case
when n < 0
  puts :negative
when n == 0
  puts :zero
else
  puts :positive
end

x = :z
case x
when :a
  puts :got_a
else
  puts :fallback
end
