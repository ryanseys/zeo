values = ["<url>"]
result = [].tap do |words|
  words << "Usage:"
  words.push(*values.map { _1 })
end.join(" ")
raise unless result == "Usage: <url>"
puts "ok"
