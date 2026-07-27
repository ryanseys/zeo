case "hello world"
when /(h\w+) (w\w+)/
  p Regexp.last_match(1)
  p Regexp.last_match(2)
  p $1
  p $~.nil?
end

case "abc"
when /x/
  puts "no"
when /(b)(c)/
  p Regexp.last_match(1)
  p $2
end

x = ["str", 42]
x.each do |v|
  case v
  when /(t)r/
    p $1
  else
    p v
  end
end

case "zzz"
when /(q)/
  puts "no"
else
  p $~.nil?
end
puts "done"

p(/(y)m/ === :sym)
p $1
case :abc
when /(b)/
  p $1
end
puts "end"
