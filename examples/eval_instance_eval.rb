# The STRING form of `instance_eval` (#97 stage 2): the source runs through the
# eval VM with `self` rebound to the receiver, so `@ivar` reads and
# implicit-self calls resolve against that object -- the whole point of
# `instance_eval`. (The block form, `obj.instance_eval { ... }`, is unchanged.)

class Account
  def initialize(balance)
    @balance = balance
    @currency = "USD"
  end
end

acct = Account.new(100)

# read the receiver's ivars from an eval'd string
puts acct.instance_eval("@balance")
puts acct.instance_eval("@currency")
puts acct.instance_eval("@balance * 2")

# interpolate the receiver's state
puts acct.instance_eval('"#{@balance} #{@currency}"')

# mutate an ivar through instance_eval, then observe it
acct.instance_eval("@balance = @balance + 50")
puts acct.instance_eval("@balance")

# instance_eval on a literal receiver: self is that object
puts "hello".instance_eval("upcase.reverse")
puts [5, 3, 1, 4, 2].instance_eval("sort.inspect")

# the block form still works alongside the string form
puts acct.instance_eval { @currency }

# top-level eval shares the main object's ivars
@note = "top-level"
puts eval("@note")
