# A class method called on a subclass must specialize through a non-overriding
# intermediate, so an implicit-self chain reaching an override resolves to the
# subclass version, not the abstract base (#1451).
class Base
  def self.table_name
    raise "#{name}.table_name must be overridden"
  end
  def self.adapter_all
    "[" + table_name + "]"
  end
  def self.all
    adapter_all          # transitive: all -> adapter_all -> table_name
  end
  def self.last
    "last-of-" + all     # one more hop
  end
end
class Article < Base
  def self.table_name; "articles"; end
end
class User < Base
  def self.table_name; "users"; end
end
puts Article.last
puts User.last
puts Article.all
puts User.adapter_all

# the direct (1-hop) case must keep working
class B2
  def self.label;    raise "abstract"; end
  def self.describe; label; end
end
class S2 < B2
  def self.label; "s2-label"; end
end
puts S2.describe
