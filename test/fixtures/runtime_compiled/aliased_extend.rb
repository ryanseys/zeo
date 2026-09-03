module AliasedExtend
  def collect_impl(names)
    names.each { |n| record_impl(n) }
  end
  def record_impl(n)
    (@collected ||= []) << "#{n}@#{self}"
  end
  alias collect collect_impl
  alias record record_impl
  def collected = @collected
end

class UsesAliasedExtend
  extend AliasedExtend
  collect([:a, :b])
  record(:c)
end
