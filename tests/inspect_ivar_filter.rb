# Kernel#instance_variables_to_inspect: nil = every ivar; an Array is a
# MEMBERSHIP filter over the object's own ivar order (oracle-pinned: the
# array's order does not reorder, strings never match, non-Array raises).
p Kernel.private_instance_methods(false).include?(:instance_variables_to_inspect)

class Plain
  def initialize = (@a = 1)
end
p Plain.new.inspect.sub(/0x[0-9a-f]+/, "0xADDR")

class Filtered
  def initialize
    @a = 1
    @b = 2
    @secret = "s3cr3t"
  end
  private def instance_variables_to_inspect = [:@b, :@a, :@missing]
end
p Filtered.new.inspect.sub(/0x[0-9a-f]+/, "0xADDR")

class Hidden
  def initialize = (@a = 1)
  private def instance_variables_to_inspect = []
end
p Hidden.new.inspect.sub(/0x[0-9a-f]+/, "0xADDR")

class Strings
  def initialize = (@a = 1)
  private def instance_variables_to_inspect = ["@a"]
end
p Strings.new.inspect.sub(/0x[0-9a-f]+/, "0xADDR")

class Wrong
  def initialize = (@a = 1)
  private def instance_variables_to_inspect = 5
end
begin
  Wrong.new.inspect
rescue TypeError => e
  puts e.message
end
