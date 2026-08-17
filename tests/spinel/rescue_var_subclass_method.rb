# A rescue arm binds its variable to the named exception subclass when that
# class has state or behaviour of its own. Only ivars counted, so a subclass
# whose body is a method was typed as the plain exception and the method could
# not be called. A two-level chain missed on top of that: the parent links are
# not resolved yet at that point, so the walk stopped at the subclass.
class AErr < StandardError; end

class BErr < AErr
  def code; 7; end
end

begin
  raise BErr
rescue BErr => eb
  p eb.code
end

class CErr < StandardError
  def code; 9; end
  def label; "c"; end
end

begin
  raise CErr, "boom"
rescue CErr => ec
  p ec.code
  p ec.label
  p ec.message
  p ec.class
end

# a subclass carrying ivars still specializes
class DErr < StandardError
  def initialize(m, n)
    super(m)
    @n = n
  end
  def n; @n; end
end

begin
  raise DErr.new("m", 3)
rescue DErr => ed
  p ed.n
  p ed.message
end

# a plain subclass keeps the exception face
class EErr < StandardError; end
begin
  raise EErr, "plain"
rescue EErr => ee
  p ee.message
  p ee.class
end
