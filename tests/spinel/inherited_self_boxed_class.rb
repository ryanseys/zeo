$o007 = []

class Base007
  def register007
    $o007 << self
  end
  def visible007?
    true
  end
  def kind007
    "base"
  end
end

class Sub007 < Base007
  def kind007
    "sub"
  end
end

Sub007.new.register007
Base007.new.register007
p $o007.map { |x| x.visible007? }
p $o007.map { |x| x.kind007 }
p $o007.map { |x| x.class.to_s }
