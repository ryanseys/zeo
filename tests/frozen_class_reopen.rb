class Fz; end
Fz.freeze
begin
  class Fz
    def x; end
  end
rescue FrozenError => e
  p e.class
end
p Fz.frozen?
p Fz.instance_methods(false)
