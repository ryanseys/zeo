def show(u)
  label = (u.details || "~")
  puts label
rescue NoMethodError => e
  puts "caught"
end
show(nil)
