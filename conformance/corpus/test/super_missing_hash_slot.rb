class Rec
  def as_json
    h = super
    h[:x] = 1
    h
  end
end
begin
  puts Rec.new.as_json.length
rescue NoMethodError => e
  puts "raised"
  puts e.message.include?("as_json")
end
