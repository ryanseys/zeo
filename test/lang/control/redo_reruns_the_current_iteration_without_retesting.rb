i = 0
attempts = 0
while i < 3
  attempts += 1
  if attempts < 5 && i == 1
    redo
  end
  i += 1
end
puts i
puts attempts
__END__
3
6
