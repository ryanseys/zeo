puts "hello world" =~ /world/
puts "hello world" =~ /xyz/
puts "hello".match?(/l+/)
puts(/l+/.match?("hello"))
puts("hello" !~ /xyz/)

m = "hello world".match(/(\w+) (\w+)/)
puts m[0]
puts m[1]
puts m[2]
puts m.pre_match
puts m.post_match
puts m.to_a
puts m.captures

named = "John Smith".match(/(?<first>\w+) (?<last>\w+)/)
puts named[:first]
puts named["last"]
h = named.named_captures
puts h["first"]
puts h["last"]

puts "HELLO".match?(/hello/i)
puts "HELLO".match?(/hello/)

puts("line1\nline2" =~ /^line2/)
multiline_str = "abc\ndef"
puts(multiline_str =~ /c.d/)
puts(multiline_str =~ /c.d/m)

r = /abc/x
puts r.to_s
puts(/abc/mi.inspect)

puts "one two three".scan(/\w+/)
puts "a1b2c3".scan(/([a-z])(\d)/)

puts "a,b,,c".split(/,/)
puts ",a,b".split(/,/)

puts "hello world".gsub(/o/, "0")
puts "hello world".sub(/o/, "0")
puts "John Smith".gsub(/(\w+) (\w+)/, '\2 \1')
puts "hello".gsub(/l/) { |match| "[#{match}]" }

count = 0
"one two three".gsub(/\w+/) { |w| count += 1; w }
puts count

puts(/foo/ === "foobar")
puts(/foo/ === "baz")

class Classifier
  def classify(x)
    case x
    when /^\d+$/
      "number"
    when /^[a-z]+$/
      "lower"
    else
      "other"
    end
  end

  def classify_in(x)
    case x
    in /^\d+$/
      "number"
    in /^[a-z]+$/
      "lower"
    else
      "other"
    end
  end
end
classifier = Classifier.new
puts classifier.classify("123")
puts classifier.classify("abc")
puts classifier.classify("ABC")
puts classifier.classify_in("123")
puts classifier.classify_in("abc")

word = "wor"
interpolated = /#{word}ld/
puts interpolated.match?("world")
puts interpolated.source

percent = %r{foo/bar}
puts percent.match?("xxfoo/barxx")

r2 = /abc/
puts r2.is_a?(Regexp)
puts r2.is_a?(Object)
puts m.is_a?(MatchData)

bad = "("
begin
  invalid = /#{bad}/
  puts "no error"
rescue RegexpError => _e
  puts "regexp error"
end
