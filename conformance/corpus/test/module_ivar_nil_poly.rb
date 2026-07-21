# A module-level instance variable initialized to nil and later widened
# to a polymorphic value (it holds nil, an empty [], and a String push)
# must box the nil initializer as sp_box_nil(), not a bare 0, or the
# assignment to the sp_RbVal const slot fails to compile.
module Store
  @log = nil

  def self.capture
    prev = @log
    @log = []
    @log.push("select 1")
    @log = prev
  end
end

Store.capture
puts "ok"
