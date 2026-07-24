# A String|Hash parameter that this program only ever calls with a String.
# `opts` is therefore monomorphically String, so the `else` (Hash) branch is
# statically dead. `String#each` is a genuine NoMethodError in Ruby and never
# runs here (CRuby always takes the is_a?(String) branch), so it lowers to a
# runtime stub via the NoMethodError gate rather than aborting compilation (#1434).
class Conn
  def initialize(opts)
    @kind = ""
    if opts.is_a?(String)
      @kind = "string:" + opts
    else
      opts.each { |k, v| @kind = @kind + k + "=" + v }
    end
  end
  def kind; @kind; end
end
puts Conn.new("hello").kind
puts Conn.new("world").kind
