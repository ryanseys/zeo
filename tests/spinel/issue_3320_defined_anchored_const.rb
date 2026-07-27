module Underscore
  module Rails
    if defined?(::Rails)
      puts "guard TRUE"
    else
      puts "guard false"
    end
  end
end

module Toplevel1
end
module Wrap
  module Toplevel1
    p defined?(::Toplevel1)
    p defined?(::Wrap)
    p defined?(::NotDefinedAnywhere)
  end
end
