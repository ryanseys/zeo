def thrower; throw :done, [1, 2]; end
p catch(:done) { thrower }
outer = Object.new; inner = Object.new
p (catch(outer) { catch(inner) { throw outer, :to_outer }; :nr })
begin
  throw :nope, 5; puts "WRONG"
rescue UncaughtThrowError => e
  puts e.message
end
catch(:gone) { }
begin
  throw :gone
rescue UncaughtThrowError => e
  puts "gone: #{e.message}"
end
__END__
[1, 2]
:to_outer
uncaught throw :nope
gone: uncaught throw :gone
