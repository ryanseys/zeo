s = "abc"
s.freeze
begin
  s[0] = "z"
rescue FrozenError => e
  puts e.send(:message)
end
puts s
__END__
can't modify frozen String: "abc"
abc
