begin
  require ["greeter", ""].first
rescue LoadError => e
  puts e.message
end
