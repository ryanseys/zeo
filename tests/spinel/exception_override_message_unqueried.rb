class Ov7 < StandardError
  def message; "overridden"; end
end
begin; raise Ov7; rescue Ov7 => x; end
p :done
begin; raise Ov7; rescue => y; p y.message; end
p(Ov7.new.message)
