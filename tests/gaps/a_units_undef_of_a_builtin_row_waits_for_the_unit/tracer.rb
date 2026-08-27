module Debugger
  class ::Module
    undef method_added
    def method_added(mid); end
  end
end
