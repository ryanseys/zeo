class Memcache
  def get = "get a\r\n"
end

if __FILE__ == $0
  class TestConnection < Memcache
    def initialize = raise("the self-test block ran")
  end

  Memcache.new.get
end
