# `alias`/`alias_method` may name an attr_reader/attr_accessor-generated
# method as the source. The new name then reads the same backing ivar,
# carrying the ivar's inferred type (so typed uses like upcase work).
class Token
  attr_reader :access_token
  attr_accessor :scope
  alias value access_token
  alias_method :token2, :access_token
  alias scope_alias scope

  def initialize
    @access_token = "tok"
    @scope = "read"
  end
end

t = Token.new
puts t.value
puts t.token2
puts t.value.upcase          # type must propagate: string method
puts "#{t.value}!"           # interpolation
puts t.scope_alias

# Aliasing an inherited attr_reader from the parent class.
class Base
  attr_reader :name
  def initialize(n)
    @name = n
  end
end

class Derived < Base
  alias title name
end

d = Derived.new("spinel")
puts d.title
puts d.title.upcase
