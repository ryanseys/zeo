class CompileTimeSingleAttribute
  attribute :name

  def initialize(name)
    @name = name
  end
end

obj = CompileTimeSingleAttribute.new("Ada")
puts obj.name
obj.name = "Grace"
puts obj.name
