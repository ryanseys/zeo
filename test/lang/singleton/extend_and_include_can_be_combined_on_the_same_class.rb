module Ext
  def helper(x)
    x + 1
  end
end
module Inc
  def instance_helper
    "inc"
  end
end
class Both
  extend Ext
  include Inc
end
puts Both.helper(9)
puts Both.new.instance_helper
__END__
10
inc
