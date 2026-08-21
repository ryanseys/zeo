# Required by `backtrace_names_the_scope_and_the_call_line.rb`. Its own
# top level is a scope of its own -- ruby calls it `<top (required)>` --
# even though zeo SPLICES this file into the requiring one.
NESTED_BLOCKS = lambda do
  [1].each do
    raise "raised two blocks deep at a required file's top level"
  end
end

def a_method_here
  raise "raised in a method defined in a required file"
end

class Chained
  def self.boom = raise("raised through a chained call")
  def self.itself_ = self
end
