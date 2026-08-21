# CRuby names the offending value in a coercion TypeError by its CLASS, except
# nil / true / false, which it spells as themselves. spinel used the class name
# for all of them, so `[[]].sum {}` reported "NilClass can't be coerced into
# Integer" where CRuby says "nil can't be coerced into Integer".
#
# The two messages disagree about Symbol, and deliberately: "no implicit
# conversion" names the CLASS, while "can't be coerced" spells the symbol.
def add(a, b)
  a + b
end

[nil, true, false, "x", [1], :s, 1.5, {}].each do |v|
  begin
    add(1, v)
  rescue => e
    puts "#{e.class}: #{e.message}"
  end
end

[nil, true, :s].each do |v|
  begin
    add(1.5, v)
  rescue => e
    puts "#{e.class}: #{e.message}"
  end
end

# the String and Array receivers take the other message
[nil, true, :s, 1, [1]].each do |v|
  begin
    add("x", v)
  rescue => e
    puts "#{e.class}: #{e.message}"
  end
end

[nil, true, :s, "q"].each do |v|
  begin
    add([1], v)
  rescue => e
    puts "#{e.class}: #{e.message}"
  end
end

# which is how a fold over nil block values reports itself
begin
  p [[]].sum {}
rescue TypeError => e
  puts e.message
end
