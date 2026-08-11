# An `alias` written BEFORE the class overrides its source binds the body
# that existed at the alias's position -- the INHERITED one. rspec-core's
# QueryOptimized does `alias find_items_for items_for` then redefines
# `items_for` to memoize through the alias; binding the override instead
# made the memo call itself forever.
class Query
  def items_for(m) = "inherited scan #{m}"
end
class QueryOptimized < Query
  alias find_items_for items_for
  private :find_items_for
  def items_for(m) = "memoized: #{send(:find_items_for, m)}"
end
p QueryOptimized.new.items_for(1)
