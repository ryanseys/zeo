out = []
i = 0
while i < 8
  i = i + 1
  case i % 3
  when 0
    next
  end
  out << i
end
p out
__END__
[1, 2, 4, 5, 7, 8]
