# `defined?(::Rails)` inside `Underscore::Rails` asks about the top-level one,
# not the enclosing module of the same name.
# (spinel issue #3320)
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
__END__
guard false
"constant"
"constant"
nil
