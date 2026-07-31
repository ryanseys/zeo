# `instance_variables` and the default `inspect` report ivars in the order they
# were first ASSIGNED, which two instances of one class can disagree about.
class S
  def initialize(reverse)
    if reverse
      @b = 2
      @a = 1
    else
      @a = 1
      @b = 2
    end
  end
end
p S.new(false).instance_variables
p S.new(true).instance_variables
# The address in an object's default inspect is not reproducible.
def shape(o) = o.inspect.sub(/0x\h+/, "0xX")
p shape(S.new(false))
p shape(S.new(true))

# Removing an ivar and assigning it again moves it to the END.
class T
  def initialize
    @a = 1
    @b = 2
    @c = 3
  end

  def shuffle
    remove_instance_variable(:@a)
    @a = 9
  end
end
t = T.new
p t.instance_variables
t.shuffle
p t.instance_variables
p shape(t)

# An invented ivar takes its place in the same sequence.
u = T.new
u.instance_variable_set(:@z, 26)
u.shuffle
p u.instance_variables
p u.dup.instance_variables

# Assigning nil still counts as an assignment; reading never does.
class V
  def initialize(assign)
    @first = nil if assign
    @second = 2
  end

  def peek = @first
end
v = V.new(false)
p v.peek
p v.instance_variables
p V.new(true).instance_variables
