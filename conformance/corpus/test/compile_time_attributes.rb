class CompileTimeAttributeHolder
  attributes :name, :count

  def initialize(name, count)
    @name = name
    @count = count
  end
end

holder = CompileTimeAttributeHolder.new("Ada", 2)
puts holder.name
puts holder.count
holder.count = 5
puts holder.count
