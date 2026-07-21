# A regex match-capture used as the key of an instance-variable hash,
# inside a loop, must store the capture as a boxed String key, so keys
# come back as Strings and lookups by a String literal find them --
# not stored unboxed as a raw integer/pointer. The "parse lines ->
# build a lookup table in an ivar" shape (afm, i18n-country-translations).
class Font
  def initialize(text)
    @table = {}
    text.each_line do |line|
      if (m = line.match(/^(\w+) (.+)/))
        @table[m[1]] = m[2]
      end
    end
  end

  def keys
    @table.keys
  end

  def [](name)
    @table[name]
  end
end

f = Font.new("FontName Times\nWeight Bold\nItalicAngle zero")
p f.keys
p f["FontName"]
p f["Weight"]
p f["ItalicAngle"]
p f["Missing"]

# Value derived from the same capture (downcase): still a String hash.
class Index
  def initialize(text)
    @h = {}
    text.each_line do |line|
      if (m = line.match(/^(\w+)/))
        @h[m[1]] = m[1].downcase
      end
    end
  end
  def lookup(k); @h[k]; end
  def size; @h.keys.length; end
end

i = Index.new("Alpha\nBeta\nGamma")
p i.lookup("Beta")
p i.size
