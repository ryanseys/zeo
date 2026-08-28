def helper
  "helper"
end

private :helper
puts "private with a name: ok"

private
puts "bare private: ok"

def public_one
  "public"
end
public :public_one
puts "public with a name: ok"
puts self.public_one
