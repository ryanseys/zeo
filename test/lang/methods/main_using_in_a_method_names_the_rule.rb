# `using` called from a method body activates nothing: ruby raises
# RuntimeError "main.using is permitted only at toplevel", whether the
# module is anonymous or a constant a refinement actually lives in.
module Second
  refine Array do
    def second = self[1]
  end
end

def use_anonymous
  using Module.new
end

def use_named
  using Second
end

def self.use_from_a_class_method
  using Second
end

[:use_anonymous, :use_named, :use_from_a_class_method].each do |name|
  send(name)
rescue Exception => e
  puts "#{name}: #{e.class}: #{e.message}"
end

# Nothing was activated, so the refinement is still invisible here.
begin
  [1, 2].second
rescue NoMethodError
  puts "still refined nowhere"
end
__END__
use_anonymous: RuntimeError: main.using is permitted only at toplevel
use_named: RuntimeError: main.using is permitted only at toplevel
use_from_a_class_method: RuntimeError: main.using is permitted only at toplevel
still refined nowhere
