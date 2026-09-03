class Detailed < StandardError
  def initialize(field)
    @field = field
    super("invalid #{field}")
  end
  attr_reader :field
end
begin
  raise Detailed.new("email")
rescue Detailed => e
  p [e.message, e.field]
end
__END__
["invalid email", "email"]
