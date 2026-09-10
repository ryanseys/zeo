# #2453: an array-literal aggregate in argument position (Math.log([..].max, b))
# must wrap its builder in a statement-expression, not leak it into the call.
class S
  def initialize(score); @score = score; end
  def order; Math.log([(@score + 1).abs + 2, 1].max, 10); end
end
puts S.new(5).order.round(3)

# #2455: a user module named Random must not clash with the runtime sp_Random.
module OpenSSL
  module Random
    def self.random_bytes(n)
      out = ""
      i = 0
      while i < n
        # 1..127 (single-byte ASCII, never 0): this test exercises module
        # reopening + dispatch, not binary strings. A 0 byte would embed a NUL
        # and a 128..255 byte would start a multi-byte UTF-8 run, so #length
        # would count characters rather than bytes. ASCII keeps one byte to
        # one character, which makes #length deterministic here.
        out = out + (rand(127) + 1).chr
        i += 1
      end
      out
    end
  end
end
puts OpenSSL::Random.random_bytes(4).length

# #2456: Time minus a poly value (holding a Time) is a Float duration.
class A2
  def stamp; "2020-01-01"; end
end
class B2
  def stamp; Time.at(0).utc; end
end
def time_ago(time); (Time.now.utc - time).to_i > 0; end
def show(x); time_ago(x.stamp); end
puts show(B2.new)
puts (show(A2.new) rescue "raised")
__END__
0.903
4
true
raised
