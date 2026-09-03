begin
  load "./w.rb", true
rescue LoadError => e
  puts e.message
end
