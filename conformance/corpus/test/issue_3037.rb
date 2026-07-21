begin
  undefined_bareword_thing
rescue NameError => e
  puts "caught #{e.class}"
  puts e.message
end

class K
  def probe
    maybe_defined_helper
  rescue NameError
    "fallback"
  end
end
p K.new.probe

# a defined bareword still resolves normally
def real_helper; 42; end
p real_helper
