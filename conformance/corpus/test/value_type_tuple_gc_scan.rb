class NameTag
  attr_reader :text

  def initialize(text)
    @text = text
  end
end

def tag_pair(i)
  return NameTag.new("label" + i.to_s), "pair" + i.to_s
end

pair = nil
i = 0
while i < 2000
  pair = tag_pair(i)
  i = i + 1
end

junk = []
i = 0
while i < 5000
  junk << "garbage" + i.to_s
  i = i + 1
end

puts pair[0].text
puts pair[1]
