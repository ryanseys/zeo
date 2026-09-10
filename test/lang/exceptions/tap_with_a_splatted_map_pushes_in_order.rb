# `[].tap` with a splatted map inside it: the pushes land in order and the
# join answers them joined.
values = ["<url>"]
result = [].tap do |words|
  words << "Usage:"
  words.push(*values.map { _1 })
end.join(" ")
raise unless result == "Usage: <url>"
puts "ok"
__END__
ok
