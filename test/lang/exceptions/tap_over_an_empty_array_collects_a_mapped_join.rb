# `[].tap { |r| r << values.map { _1 }.join(", ") }.join(" ")` answers the single mapped element.
# (spinel issue #3200)
class Item
  attr_reader :values
  def initialize = @values = ["a"]
  def label
    [].tap do |result|
      result << values.map { _1 }.join(", ")
    end.join(" ")
  end
end
raise unless Item.new.label == "a"
puts "ok"
__END__
ok
