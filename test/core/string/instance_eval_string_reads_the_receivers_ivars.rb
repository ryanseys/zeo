class Account
  def initialize(n)
    @balance = n
  end
end
acct = Account.new(100)
src = "@balance + 5"
puts acct.instance_eval(src)
__END__
105
