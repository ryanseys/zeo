# `.name`, `.inspect`, `==`, `!=` and `eql?` on a class value: named
# directly, held in a local, and read back from an instance's `.class`. A
# subclass answers its own class, not the base's.

class Row
  attr_accessor :id
  def initialize
    @id = 0
  end
end

class Article < Row
end

# Direct class literal.
puts Row.name                 # Row
puts Article.inspect          # Article

# Through obj.class.
r = Row.new
a = Article.new
puts r.class.name             # Row
puts a.class.inspect          # Article

# Equality / inequality / eql?
puts (Row == Row).to_s        # true
puts (Row == Article).to_s    # false
puts (Row != Article).to_s    # true
puts (Row != Row).to_s        # false
puts Row.eql?(Row).to_s       # true
puts Row.eql?(Article).to_s   # false

# Through obj.class chains.
puts (r.class == Row).to_s            # true
puts (r.class == Article).to_s        # false
puts (a.class != Row).to_s            # true
puts (a.class == Article).to_s        # true
puts r.class.eql?(Row).to_s           # true
__END__
Row
Article
Row
Article
true
false
true
false
true
false
true
false
true
true
true
