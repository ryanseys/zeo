h = { a: 1 }
h.freeze
begin
  h[:b] = 2
rescue FrozenError => e
  puts e.send(:message)
end
puts h.length
__END__
can't modify frozen Hash: {a: 1}
1
